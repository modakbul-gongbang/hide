//! A node's projects keep their issues when the core runs on another
//! machine (PRD core-host-node-move D-29): a GitHub project's issues are read
//! by the repository the node names, with this machine's login; a Local
//! project's are this core's store under the node's id, the slot a core move
//! writes the machine giving up the core into; and a checkout's link is read
//! and written on the node. A device this core dials keeps none here.
//!
//! The node is the documents test double, which answers with the real host
//! dispatch, so its facts, worktrees and branch notes are real Git.

use super::device_catalog::{herdr_workspace, session};
use super::device_worktrees::git;
use super::documents::FakeDevice;
use super::*;
use crate::node_access::{LinkAnswer, LinkError, NodeLink};
use hide_node_link::protocol::Call;

const NODE: &str = "macbook-node";

/// A link that keeps the Git and `gh` calls it is asked, answering through
/// `inner`, or refusing them when it has none: this machine's node in a test
/// that proves a node's work never lands here.
struct Recording {
    inner: Option<Arc<dyn NodeLink>>,
    calls: Mutex<Vec<Call>>,
    gh_stdout: Mutex<Option<String>>,
}

impl Recording {
    fn of(inner: Option<Arc<dyn NodeLink>>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            calls: Mutex::new(Vec::new()),
            gh_stdout: Mutex::new(None),
        })
    }

    fn git_writes(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|call| match call {
                Call::Git { root, command } => {
                    let shown = format!("{command:?}");
                    shown
                        .contains("BranchConfig")
                        .then(|| format!("{root} {shown}"))
                }
                _ => None,
            })
            .collect()
    }

    fn gh_calls(&self) -> Vec<(Option<String>, Option<String>, Vec<String>)> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|call| match call {
                Call::Gh {
                    cwd,
                    args,
                    repository,
                } => Some((cwd.clone(), repository.clone(), args.clone())),
                _ => None,
            })
            .collect()
    }
}

impl NodeLink for Recording {
    fn call(&self, call: Call, timeout: Duration) -> Result<LinkAnswer, LinkError> {
        self.calls.lock().unwrap().push(call.clone());
        if matches!(call, Call::Gh { .. })
            && let Some(stdout) = self.gh_stdout.lock().unwrap().clone()
        {
            return Ok(LinkAnswer::Parsed(
                serde_json::to_value(hide_node_link::gh::GhAnswer::Output { stdout }).unwrap(),
            ));
        }
        match &self.inner {
            Some(inner) => inner.call(call, timeout),
            None => Err(LinkError::Refused(hide_node_link::HostError::new(
                hide_node_link::ErrorCode::InvalidRequest,
                "this machine's node takes no call in this test".to_owned(),
            ))),
        }
    }
}

struct Repo {
    _dir: tempfile::TempDir,
    root: String,
    linked: String,
}

/// A repository on `main` with a linked worktree on `branch`, and `origin`
/// set to `origin` when given.
fn repo(origin: Option<&str>, branch: &str) -> Repo {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let root = base.join("app").to_string_lossy().into_owned();
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
    if let Some(origin) = origin {
        git(&root, &["remote", "add", "origin", origin]);
    }
    git(&root, &["worktree", "add", "-q", "-b", branch, &linked]);
    Repo {
        _dir: dir,
        root,
        linked,
    }
}

/// A runtime a node dials in to, whose session holds the repository's two
/// checkouts, with the node's facts, worktrees and branch notes read through
/// its link. This machine's node is `own`.
fn node_runtime(
    repo: &Repo,
    node: Arc<dyn NodeLink>,
    own: Arc<dyn NodeLink>,
    store: crate::local_issues::LocalIssueStore,
) -> SharedRuntime {
    let mut runtime = runtime();
    runtime.own_node = own;
    runtime.local_issues = Ok(store);
    runtime
        .snapshot
        .ui_state
        .device_registrations
        .push(crate::model::DeviceRegistration {
            id: NODE.to_owned(),
            label: "MacBook".to_owned(),
            origin: crate::model::LinkOrigin::Inbound,
            herdr_socket_path: None,
            host_consent: None,
        });
    runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
        target_id: NODE.to_owned(),
        state: "connected".to_owned(),
        message: None,
        herdr_version: None,
        session: None,
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
    });
    runtime.device_hosts.insert(
        NODE.to_owned(),
        hosts::DeviceHost {
            phase: hosts::HostPhase::Ready {
                host: node,
                platform: "macos aarch64".to_owned(),
                helper_path: Some("/fake/hided".to_owned()),
            },
            generation: 1,
        },
    );
    let shared = SharedRuntime::new(runtime);
    {
        let mut runtime = shared.lock().unwrap();
        let connector: Arc<dyn hide_herdr_client::ApiConnector> = Arc::new(
            hide_herdr_client::LocalSocketConnector::new("/tmp/herdr-core-never-connect.sock"),
        );
        runtime.install_remote_control(RemoteControlContext::new(
            NODE,
            connector,
            shared.weak(),
            ChangeNotifier::noop(),
        ));
        runtime.install_worker_context(shared.weak(), ChangeNotifier::noop());
        let raw = session(vec![
            herdr_workspace(NODE, "w1", &repo.root, &[("t1", &repo.root)]),
            herdr_workspace(NODE, "w2", &repo.linked, &[("t2", &repo.linked)]),
        ]);
        runtime.ingest_remote_session(NODE, Ok(raw));
    }
    let (root, linked) = (repo.root.clone(), repo.linked.clone());
    wait(
        &shared,
        "the node's worktrees and branch notes",
        move |runtime| {
            runtime
                .device_worktrees
                .get(NODE)
                .is_some_and(|listed| listed.projects.contains_key(&root))
                && node_project(runtime).is_some_and(|project| {
                    project
                        .checkouts
                        .iter()
                        .any(|checkout| checkout.path == linked && checkout.branch.is_some())
                })
        },
    );
    shared
}

fn node_project(runtime: &Runtime) -> Option<&WorkspaceSnapshot> {
    runtime
        .snapshot
        .status
        .remote
        .iter()
        .find(|status| status.target_id == NODE)?
        .session
        .as_ref()?
        .workspaces
        .iter()
        .find(|project| project.is_git)
}

fn dispatch(shared: &SharedRuntime, kind: &str, payload: serde_json::Value) {
    let event =
        serde_json::json!({"schema_version": SCHEMA_VERSION, "kind": kind, "payload": payload});
    shared
        .lock()
        .unwrap()
        .dispatch_json(&serde_json::to_vec(&event).unwrap());
}

fn local_project(issues: &[(u32, &str)]) -> crate::local_issues::LocalIssueProject {
    crate::local_issues::LocalIssueProject {
        next_number: issues
            .iter()
            .map(|(number, _)| number + 1)
            .max()
            .unwrap_or(1),
        issues: issues
            .iter()
            .map(|(number, title)| crate::local_issues::LocalIssue {
                number: *number,
                title: (*title).to_owned(),
                open: true,
                created_at_unix_ms: 1,
                updated_at_unix_ms: 1,
                ..Default::default()
            })
            .collect(),
    }
}

fn task_ids(project: &WorkspaceSnapshot) -> Vec<(String, String, bool)> {
    project
        .tasks
        .tasks
        .iter()
        .map(|task| (task.id.clone().unwrap(), task.title.clone(), task.open))
        .collect()
}

/// A Local project of a node shows the issues a core move kept under the
/// node's id, never this machine's project at the same path; a new issue
/// continues the node's numbers and lands there, and closing one closes the
/// node's. The checkout whose branch starts with a number names that issue.
#[test]
fn a_node_s_local_project_reads_and_writes_the_issues_a_move_kept_under_its_id() {
    let repo = repo(None, "7-moved-work");
    let mut store = crate::local_issues::LocalIssueStore::default();
    store
        .projects
        .insert(repo.root.clone(), local_project(&[(1, "this machine's")]));
    store.devices.entry(NODE.to_owned()).or_default().insert(
        repo.root.clone(),
        local_project(&[(6, "older"), (7, "moved with the core")]),
    );
    let shared = node_runtime(&repo, FakeDevice::new(), Recording::of(None), store);
    let (workspace_id, root) = {
        let runtime = shared.lock().unwrap();
        let project = node_project(&runtime).unwrap();
        assert_eq!(project.tasks.source.as_ref().unwrap().kind, "local");
        assert_eq!(
            task_ids(project),
            [
                ("L-7".to_owned(), "moved with the core".to_owned(), true),
                ("L-6".to_owned(), "older".to_owned(), true),
            ]
        );
        let linked = project
            .checkouts
            .iter()
            .find(|checkout| checkout.path == repo.linked)
            .unwrap();
        let key = crate::tasks::local_key(&repo.root, 7);
        assert_eq!(linked.task_key.as_deref(), Some(key.as_str()));
        (project.id.clone(), project.path.clone())
    };

    dispatch(
        &shared,
        "issue_create",
        serde_json::json!({"workspace_id": workspace_id, "title": "made on the new core"}),
    );
    let runtime = shared.lock().unwrap();
    let created = runtime.snapshot.issue_work.create.clone().unwrap();
    assert_eq!(created.phase, "ready", "{:?}", created.message);
    assert_eq!(created.task_key, Some(crate::tasks::local_key(&root, 8)));
    assert_eq!(task_ids(node_project(&runtime).unwrap())[0].0, "L-8");
    drop(runtime);

    dispatch(
        &shared,
        "issue_set_open",
        serde_json::json!({"workspace_id": workspace_id, "task_key": crate::tasks::local_key(&root, 8), "open": false}),
    );
    let runtime = shared.lock().unwrap();
    let store = runtime.local_issues.as_ref().unwrap();
    let node = store.project(Some(NODE), &root).unwrap();
    assert_eq!(node.next_number, 9);
    assert!(!store.issue(Some(NODE), &root, 8).unwrap().open);
    assert_eq!(store.project(None, &root).unwrap().issues.len(), 1);
    assert!(store.issue(None, &root, 1).unwrap().open);
    assert!(!task_ids(node_project(&runtime).unwrap())[0].2);
}

/// A key names only a path, so an edit or a close names its project too, and
/// a key of a path the named project does not have is refused rather than
/// applied to another machine's project at that path.
#[test]
fn a_local_issue_key_is_applied_only_to_the_project_the_request_names() {
    let repo = repo(None, "feature");
    let mut store = crate::local_issues::LocalIssueStore::default();
    store
        .devices
        .entry(NODE.to_owned())
        .or_default()
        .insert(repo.root.clone(), local_project(&[(3, "node issue")]));
    store
        .projects
        .insert("/elsewhere".to_owned(), local_project(&[(3, "other path")]));
    let shared = node_runtime(&repo, FakeDevice::new(), Recording::of(None), store);
    let workspace_id = node_project(&shared.lock().unwrap()).unwrap().id.clone();
    dispatch(
        &shared,
        "local_issue_update",
        serde_json::json!({"request_id": "r1", "workspace_id": workspace_id, "task_key": "local:/elsewhere#3", "title": "moved"}),
    );
    let runtime = shared.lock().unwrap();
    let update = runtime.snapshot.issue_work.update.clone().unwrap();
    assert_eq!(update.phase, "failed");
    let store = runtime.local_issues.as_ref().unwrap();
    assert_eq!(
        store.issue(None, "/elsewhere", 3).unwrap().title,
        "other path"
    );
    drop(runtime);
    dispatch(
        &shared,
        "local_issue_update",
        serde_json::json!({"request_id": "r2", "workspace_id": workspace_id, "task_key": crate::tasks::local_key(&repo.root, 3), "title": "renamed on the node"}),
    );
    let runtime = shared.lock().unwrap();
    assert_eq!(
        runtime.snapshot.issue_work.update.as_ref().unwrap().phase,
        "ready"
    );
    let store = runtime.local_issues.as_ref().unwrap();
    assert_eq!(
        store.issue(Some(NODE), &repo.root, 3).unwrap().title,
        "renamed on the node"
    );
}

fn github_issue(repository: &str, number: u32, title: &str) -> crate::issues::IssueSnapshot {
    crate::issues::IssueSnapshot {
        reference: crate::issues::IssueReference {
            repository: repository.to_owned(),
            number,
        },
        title: title.to_owned(),
        url: format!("https://github.com/{repository}/issues/{number}"),
        state: "OPEN".to_owned(),
        project_status: None,
        created_at_unix_ms: None,
        updated_at_unix_ms: None,
        closed_at_unix_ms: None,
        blocked_by: Vec::new(),
        sub_issues: Default::default(),
        labels: Vec::new(),
    }
}

/// A GitHub project of a node lists the issues this machine's read of the
/// repository the node named brought, and a checkout whose branch starts
/// with a number links that issue; the node's next read asks for the linked
/// issues by name.
#[test]
fn a_node_s_github_project_lists_its_repository_s_issues_and_links_its_checkouts() {
    let repo = repo(Some("https://github.com/acme/app.git"), "12-fix-login");
    let shared = node_runtime(
        &repo,
        FakeDevice::new(),
        Recording::of(None),
        Default::default(),
    );
    let mut runtime = shared.lock().unwrap();
    let request = runtime.device_github_request(NODE, std::time::Instant::now());
    assert_eq!(request.projects.len(), 1);
    assert_eq!(request.projects[0].repository.as_deref(), Some("acme/app"));
    assert_eq!(
        request.projects[0]
            .links
            .iter()
            .map(|link| link.token())
            .collect::<Vec<_>>(),
        ["acme/app#12"]
    );
    let first = request.projects[0].generation;
    assert_eq!(
        node_project(&runtime)
            .unwrap()
            .tasks
            .source
            .as_ref()
            .map(|source| source.reading),
        Some(true)
    );
    runtime.ingest_device_github(
        NODE,
        crate::model::GithubSnapshot {
            projects: vec![crate::model::GithubProjectSnapshot {
                pull_requests_read: true,
                issues_read: true,
                issues: crate::issues::ProjectIssuesSnapshot {
                    repository: Some("acme/app".to_owned()),
                    issues: vec![
                        github_issue("acme/app", 14, "Newer"),
                        github_issue("acme/app", 12, "Fix login"),
                    ],
                    ..Default::default()
                },
                root_path: repo.root.clone(),
                status: crate::model::GithubStatusSnapshot {
                    available: true,
                    last_success_at_unix_ms: Some(5),
                    ..Default::default()
                },
                pull_requests: Vec::new(),
            }],
        },
    );
    let project = node_project(&runtime).unwrap();
    let source = project.tasks.source.clone().unwrap();
    assert_eq!(
        (source.kind.as_str(), source.name.as_deref(), source.reading),
        ("github", Some("acme/app"), false)
    );
    assert_eq!(
        project
            .tasks
            .tasks
            .iter()
            .map(|task| task.key.as_str())
            .collect::<Vec<_>>(),
        ["github:acme/app#14", "github:acme/app#12"]
    );
    let linked = project
        .checkouts
        .iter()
        .find(|checkout| checkout.path == repo.linked)
        .unwrap();
    assert_eq!(linked.task_key.as_deref(), Some("github:acme/app#12"));
    assert_eq!(linked.issue.as_ref().unwrap().source, "브랜치 이름");
    // The same links ask nothing again before the next re-read.
    let again = runtime.device_github_request(NODE, std::time::Instant::now());
    assert_eq!(again.projects[0].generation, first);
}

/// When this machine's `gh` is signed out, a node's GitHub project says so
/// with the reason, as one of this machine's does, instead of moving its
/// issues to Local or reading forever.
#[test]
fn a_node_project_says_when_this_machine_s_gh_is_signed_out() {
    let repo = repo(Some("git@github.com:acme/app.git"), "feature");
    let shared = node_runtime(
        &repo,
        FakeDevice::new(),
        Recording::of(None),
        Default::default(),
    );
    let mut runtime = shared.lock().unwrap();
    runtime.ingest_device_github(
        NODE,
        crate::model::GithubSnapshot {
            projects: vec![crate::model::GithubProjectSnapshot {
                root_path: repo.root.clone(),
                status: crate::model::GithubStatusSnapshot {
                    available: false,
                    unavailable_reason: Some("gh auth login".to_owned()),
                    failure_category: Some(crate::model::GithubFailureCategory::NotLoggedIn),
                    ..Default::default()
                },
                ..Default::default()
            }],
        },
    );
    let source = node_project(&runtime)
        .unwrap()
        .tasks
        .source
        .clone()
        .unwrap();
    assert_eq!(
        (
            source.kind.as_str(),
            source.failure.as_deref(),
            source.reading
        ),
        ("github", Some("gh auth login"), false)
    );
}

/// A new GitHub issue for a node's project is made by the repository's name
/// with this machine's login, never in a folder of this machine at the
/// node's path, and shows on the project at once.
#[test]
fn a_new_github_issue_for_a_node_project_is_made_by_repository_name_here() {
    let repo = repo(Some("https://github.com/acme/app.git"), "feature");
    let own = Recording::of(None);
    *own.gh_stdout.lock().unwrap() = Some("https://github.com/acme/app/issues/31\n".to_owned());
    let shared = node_runtime(&repo, FakeDevice::new(), own.clone(), Default::default());
    let workspace_id = {
        let mut runtime = shared.lock().unwrap();
        runtime.device_github_request(NODE, std::time::Instant::now());
        runtime.ingest_device_github(
            NODE,
            crate::model::GithubSnapshot {
                projects: vec![crate::model::GithubProjectSnapshot {
                    pull_requests_read: true,
                    issues_read: true,
                    issues: crate::issues::ProjectIssuesSnapshot {
                        repository: Some("acme/app".to_owned()),
                        ..Default::default()
                    },
                    root_path: repo.root.clone(),
                    status: crate::model::GithubStatusSnapshot {
                        available: true,
                        ..Default::default()
                    },
                    pull_requests: Vec::new(),
                }],
            },
        );
        node_project(&runtime).unwrap().id.clone()
    };
    dispatch(
        &shared,
        "issue_create",
        serde_json::json!({"workspace_id": workspace_id, "title": "From the MacBook", "body": "b"}),
    );
    wait(&shared, "the new issue", |runtime| {
        runtime
            .snapshot
            .issue_work
            .create
            .as_ref()
            .is_some_and(|create| create.phase != "working")
    });
    let runtime = shared.lock().unwrap();
    let create = runtime.snapshot.issue_work.create.clone().unwrap();
    assert_eq!(
        create.task_key.as_deref(),
        Some("github:acme/app#31"),
        "{create:?}"
    );
    assert_eq!(
        own.gh_calls(),
        [(
            None,
            Some("acme/app".to_owned()),
            [
                "issue",
                "create",
                "--title",
                "From the MacBook",
                "--body",
                "b"
            ]
            .map(str::to_owned)
            .to_vec(),
        )]
    );
    assert_eq!(
        node_project(&runtime).unwrap().tasks.tasks[0].key,
        "github:acme/app#31"
    );
}

/// A node checkout's issue link is written on the node, through its link,
/// and the core machine's git is never asked; the checkout shows the link
/// at once.
#[test]
fn a_node_checkout_s_issue_link_is_written_on_the_node_never_here() {
    let repo = repo(None, "feature");
    let mut store = crate::local_issues::LocalIssueStore::default();
    store
        .devices
        .entry(NODE.to_owned())
        .or_default()
        .insert(repo.root.clone(), local_project(&[(4, "pick me")]));
    let node = Recording::of(Some(FakeDevice::new()));
    let own = Recording::of(None);
    let shared = node_runtime(&repo, node.clone(), own.clone(), store);
    let checkout_id = {
        let runtime = shared.lock().unwrap();
        let project = node_project(&runtime).unwrap();
        let checkout = project
            .checkouts
            .iter()
            .find(|checkout| checkout.path == repo.linked)
            .unwrap();
        assert_eq!(checkout.task_key, None);
        checkout.id.clone()
    };
    dispatch(
        &shared,
        "set_checkout_issue",
        serde_json::json!({"checkout_id": checkout_id, "text": "L-4"}),
    );
    let key = crate::tasks::local_key(&repo.root, 4);
    wait(&shared, "the link on the node's checkout", |runtime| {
        node_project(runtime).is_some_and(|project| {
            project
                .checkouts
                .iter()
                .any(|checkout| checkout.task_key.as_deref() == Some(key.as_str()))
        })
    });
    assert_eq!(
        node.git_writes(),
        [format!(
            "{} SetBranchConfig {{ branch: \"feature\", key: Issue, value: \"L-4\" }}",
            repo.root
        )]
    );
    assert!(own.git_writes().is_empty());
    assert_eq!(
        git(&repo.root, &["config", "--get", "branch.feature.issue"]).trim(),
        "L-4"
    );
}

/// A device this core dials keeps its issues on that device: its projects
/// carry no issue source here, so no board, New issue or link reaches them.
#[test]
fn a_device_this_core_dials_keeps_no_issues_here() {
    let repo = repo(None, "3-work");
    let mut store = crate::local_issues::LocalIssueStore::default();
    store
        .devices
        .entry(NODE.to_owned())
        .or_default()
        .insert(repo.root.clone(), local_project(&[(3, "kept, not shown")]));
    let shared = node_runtime(&repo, FakeDevice::new(), Recording::of(None), store);
    let mut runtime = shared.lock().unwrap();
    runtime.snapshot.ui_state.device_registrations[0].origin = crate::model::LinkOrigin::Dialed {
        ssh_alias: "macbook".to_owned(),
    };
    runtime.refresh_device_catalog(NODE);
    let project = node_project(&runtime).unwrap();
    assert_eq!(project.tasks, crate::tasks::ProjectTasksSnapshot::default());
    assert!(
        project
            .checkouts
            .iter()
            .all(|checkout| checkout.task_key.is_none())
    );
    let workspace_id = project.id.clone();
    drop(runtime);
    dispatch(
        &shared,
        "issue_create",
        serde_json::json!({"workspace_id": workspace_id, "title": "nowhere"}),
    );
    let runtime = shared.lock().unwrap();
    assert_eq!(
        runtime.snapshot.status.last_error.as_ref().unwrap().kind,
        "issue_create.unknown_project"
    );
    let store = runtime.local_issues.as_ref().unwrap();
    assert_eq!(
        store.project(Some(NODE), &repo.root).unwrap().issues.len(),
        1
    );
}
