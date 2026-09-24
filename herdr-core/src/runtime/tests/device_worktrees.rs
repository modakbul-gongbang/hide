//! A device's worktrees are read, checked and deleted by that device's own
//! helper (PRD S5.5 B27-B29): a deletion is decided on the device's facts,
//! never on this machine's catalog at the same path, and its receipt names
//! the device.
//!
//! The helper is the documents test double, which answers with the real
//! host dispatch, so the Git work is real.

use super::documents::FakeDevice;
use super::*;

const DEVICE: &str = "device-w";

struct Repo {
    _dir: tempfile::TempDir,
    root: String,
    linked: String,
}

fn git(cwd: &str, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap()
}

/// A repository on `main` with a linked worktree on `feature` at the same
/// commit, so the branch is merged and may be deleted with it, and a
/// `parked` branch no worktree has checked out.
fn repo() -> Repo {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let root = base.join("repo").to_string_lossy().into_owned();
    let linked = base.join("linked").to_string_lossy().into_owned();
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(
        &root,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ],
    );
    // The default branch is read from `origin/HEAD`, as a clone has it.
    git(&root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(
        &root,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    git(&root, &["branch", "parked"]);
    git(&root, &["worktree", "add", "-q", "-b", "feature", &linked]);
    Repo {
        _dir: dir,
        root,
        linked,
    }
}

/// A runtime connected to one device whose session holds the repository,
/// with that device's worktree facts already read through its helper.
fn device_runtime(repo: &Repo) -> Arc<Mutex<Runtime>> {
    let mut runtime = runtime();
    let workspace_id = format!("remote:{DEVICE}:workspace:w1");
    let checkout_id = format!("remote:{DEVICE}:checkout:w1");
    let mut project = workspace(
        &workspace_id,
        "repo",
        &repo.root,
        vec![checkout(&workspace_id, &checkout_id, &repo.root, None)],
    );
    project.is_git = true;
    project.remote_target_id = Some(DEVICE.to_owned());
    project.device_id = DEVICE.to_owned();
    runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
        target_id: DEVICE.to_owned(),
        state: "connected".to_owned(),
        message: None,
        herdr_version: None,
        session: Some(RemoteSessionSnapshot {
            workspaces: vec![project],
            agents: Vec::new(),
            active_tab_ids: Default::default(),
            focused_workspace_id: None,
            focused_checkout_id: None,
            focused_tab_id: None,
            focused_pane_id: None,
            pane_layouts: Vec::new(),
        }),
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
    });
    runtime.device_hosts.insert(
        DEVICE.to_owned(),
        hosts::DeviceHost {
            phase: hosts::HostPhase::Ready {
                host: FakeDevice::new(),
                platform: "macos aarch64".to_owned(),
                helper_path: "/fake/hide-host-helper".to_owned(),
            },
            generation: 1,
        },
    );
    runtime.request_device_worktrees(DEVICE, true);
    let shared = Arc::new(Mutex::new(runtime));
    let mut runtime = shared.lock().unwrap();
    // No pane sits in the worktree, so the deletion never reaches Herdr.
    let connector: Arc<dyn hide_herdr_client::ApiConnector> = Arc::new(
        hide_herdr_client::UnixSocketConnector::new("/tmp/herdr-core-never-connect.sock"),
    );
    runtime.install_remote_control(RemoteControlContext::new(
        DEVICE,
        connector,
        Arc::downgrade(&shared),
        ChangeNotifier::noop(),
    ));
    runtime.install_worker_context(Arc::downgrade(&shared), ChangeNotifier::noop());
    drop(runtime);
    shared
}

fn dispatch(shared: &Arc<Mutex<Runtime>>, kind: &str, payload: serde_json::Value) {
    let event =
        serde_json::json!({"schema_version": SCHEMA_VERSION, "kind": kind, "payload": payload});
    shared
        .lock()
        .unwrap()
        .dispatch_json(&serde_json::to_vec(&event).unwrap());
}

fn wait(shared: &Arc<Mutex<Runtime>>, what: &str, ready: impl Fn(&Runtime) -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !ready(&shared.lock().unwrap()) {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn last_error(shared: &Arc<Mutex<Runtime>>) -> Option<String> {
    let runtime = shared.lock().unwrap();
    runtime
        .snapshot
        .status
        .last_error
        .as_ref()
        .map(|error| error.kind.clone())
}

/// B27, B29: this machine lists the same path with a gate that refuses it;
/// the device's deletion is decided on the device's facts, runs on its
/// helper, deletes the merged branch there, and its receipt names it.
#[test]
fn a_device_worktree_is_deleted_by_its_helper_whatever_this_machine_lists_at_that_path() {
    let repo = repo();
    let shared = device_runtime(&repo);
    {
        let mut runtime = shared.lock().unwrap();
        let mut local = runtime.device_worktrees[DEVICE].projects[&repo.root].clone();
        for row in &mut local.worktrees {
            row.deletion_gate.blocked_reason = Some("Local block".to_owned());
        }
        runtime.worktree_catalog.projects = vec![local];
    }

    dispatch(
        &shared,
        "remove_worktree",
        serde_json::json!({"checkout_path": repo.linked, "delete_branch": true}),
    );
    assert_eq!(
        last_error(&shared).as_deref(),
        Some("worktree.remove_blocked")
    );
    assert!(shared.lock().unwrap().snapshot.worktree_removal.is_none());

    dispatch(
        &shared,
        "remove_worktree",
        serde_json::json!({"device_id": DEVICE, "checkout_path": repo.linked, "delete_branch": true}),
    );
    wait(&shared, "the device removal to finish", |runtime| {
        runtime
            .snapshot
            .worktree_removal
            .as_ref()
            .is_some_and(|removal| removal.phase != "closing" && removal.phase != "removing")
    });
    let removal = shared
        .lock()
        .unwrap()
        .snapshot
        .worktree_removal
        .clone()
        .unwrap();
    assert_eq!(removal.phase, "finished", "{:?}", removal.message);
    assert_eq!(removal.device_id.as_deref(), Some(DEVICE));
    assert!(!Path::new(&repo.linked).exists());
    assert_eq!(git(&repo.root, &["branch", "--list", "feature"]), "");
    wait(&shared, "the device list to drop the worktree", |runtime| {
        runtime.device_worktrees[DEVICE].projects[&repo.root]
            .worktrees
            .iter()
            .all(|row| row.path != repo.linked)
    });
}

/// B29: a device worktree with uncommitted changes is refused on the
/// device's own facts before any pane closes.
#[test]
fn a_dirty_device_worktree_is_refused_before_anything_closes() {
    let repo = repo();
    std::fs::write(Path::new(&repo.linked).join("draft.txt"), "wip\n").unwrap();
    let shared = device_runtime(&repo);

    dispatch(
        &shared,
        "remove_worktree",
        serde_json::json!({"device_id": DEVICE, "checkout_path": repo.linked, "delete_branch": true}),
    );

    assert_eq!(
        last_error(&shared).as_deref(),
        Some("worktree.remove_blocked")
    );
    assert!(shared.lock().unwrap().snapshot.worktree_removal.is_none());
    assert!(Path::new(&repo.linked).exists());
}

/// B28: a new worktree's branch is checked by the device's helper, so a
/// branch that exists there and no worktree holds is refused with Git's own
/// answer before Herdr is asked for anything.
#[test]
fn a_device_worktree_for_a_branch_that_exists_there_is_refused_by_its_helper() {
    let repo = repo();
    let shared = device_runtime(&repo);

    dispatch(
        &shared,
        "create_worktree",
        serde_json::json!({"device_id": DEVICE, "repository_root": repo.root, "branch": "parked"}),
    );
    wait(&shared, "the create to settle", |runtime| {
        runtime
            .snapshot
            .task_operation
            .as_ref()
            .is_some_and(|operation| operation.phase != "working")
    });
    let operation = shared
        .lock()
        .unwrap()
        .snapshot
        .task_operation
        .clone()
        .unwrap();
    assert_eq!(operation.phase, "failed");
    assert_eq!(operation.device_id.as_deref(), Some(DEVICE));
    assert!(
        operation
            .message
            .as_deref()
            .is_some_and(|message| message.contains("already exists")),
        "{:?}",
        operation.message
    );
}

/// A device repository whose worktrees have not been read yet is refused as
/// unread, not as one without branches, and the read is asked for.
#[test]
fn a_device_worktree_asked_for_before_its_repository_is_read_says_so() {
    let repo = repo();
    let shared = device_runtime(&repo);
    shared.lock().unwrap().device_worktrees.clear();

    dispatch(
        &shared,
        "create_worktree",
        serde_json::json!({"device_id": DEVICE, "repository_root": repo.root, "branch": "next"}),
    );

    assert_eq!(
        last_error(&shared).as_deref(),
        Some("worktree.create_unread")
    );
    assert!(shared.lock().unwrap().snapshot.task_operation.is_none());
    wait(&shared, "the device's worktrees to be read", |runtime| {
        runtime
            .device_worktrees
            .get(DEVICE)
            .is_some_and(|worktrees| worktrees.projects.contains_key(&repo.root))
    });
}
