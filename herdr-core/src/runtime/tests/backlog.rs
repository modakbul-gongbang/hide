//! The backlog's add, retry and undo (PRD overview-device-tasks D-10, D-21,
//! D-34) against a scripted `gh`: a GitHub item is one title-only create,
//! one at a time per device with eight waiting, and a retry, even after a
//! restart, names the issue the first try made instead of making another.

use super::*;
use crate::issues::ProjectIssuesSnapshot;
use crate::model::{BacklogFailure, BacklogPhase};
use crate::node_access::{LinkAnswer, LinkError, NodeLink};
use hide_node_link::gh::{GhAnswer, GithubFailureCategory};
use hide_node_link::protocol::Call;
use std::sync::Condvar;

/// `gh` for one checkout, whose repository is `acme/project`, and for any
/// repository named outright: creates are numbered from 41 and wait while the
/// gate is shut; anything not scripted fails as offline.
struct FakeGh {
    calls: Mutex<Vec<String>>,
    open: Mutex<bool>,
    opened: Condvar,
    created: Mutex<u32>,
    /// What the next create prints instead of its URL.
    create_prints: Mutex<Option<String>>,
    /// The issues the reconcile query answers.
    reconcile: Mutex<Vec<(u32, String)>>,
}

impl FakeGh {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            open: Mutex::new(true),
            opened: Condvar::new(),
            created: Mutex::new(40),
            create_prints: Mutex::new(None),
            reconcile: Mutex::new(Vec::new()),
        })
    }

    fn gate(&self, open: bool) {
        *self.open.lock().unwrap() = open;
        self.opened.notify_all();
    }

    fn calls(&self, prefix: &str) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with(prefix))
            .cloned()
            .collect()
    }

    fn answer(&self, args: &[String], repository: Option<&str>) -> GhAnswer {
        let line = args.join(" ");
        let output = |stdout: String| GhAnswer::Output { stdout };
        if line.starts_with("repo view") {
            let name = repository.unwrap_or("acme/project");
            return output(serde_json::json!({ "nameWithOwner": name }).to_string());
        }
        if line.starts_with("issue create") {
            let mut open = self.open.lock().unwrap();
            while !*open {
                open = self.opened.wait(open).unwrap();
            }
            drop(open);
            let mut number = self.created.lock().unwrap();
            *number += 1;
            if let Some(printed) = self.create_prints.lock().unwrap().take() {
                return output(printed);
            }
            return output(format!("https://github.com/acme/project/issues/{number}\n"));
        }
        if line.starts_with("issue close") {
            return output(String::new());
        }
        if line.starts_with("api graphql") {
            let nodes: Vec<_> = self
                .reconcile
                .lock()
                .unwrap()
                .iter()
                .map(|(number, title)| {
                    serde_json::json!({
                        "number": number, "title": title, "state": "OPEN",
                        "url": format!("https://github.com/acme/project/issues/{number}"),
                        "createdAt": "2099-01-01T00:00:00Z",
                    })
                })
                .collect();
            return output(
                serde_json::json!({"data": {"repository": {"issues": {
                    "nodes": nodes, "pageInfo": {"hasNextPage": false},
                }}}})
                .to_string(),
            );
        }
        GhAnswer::Failed {
            category: GithubFailureCategory::NetworkOrRateLimit,
            reason: "not scripted".into(),
        }
    }
}

impl NodeLink for FakeGh {
    fn call(&self, call: Call, _timeout: Duration) -> Result<LinkAnswer, LinkError> {
        let Call::Gh {
            args, repository, ..
        } = call
        else {
            return Err(LinkError::NotConnected("only gh is scripted".into()));
        };
        self.calls.lock().unwrap().push(args.join(" "));
        Ok(LinkAnswer::Parsed(
            serde_json::to_value(self.answer(&args, repository.as_deref())).unwrap(),
        ))
    }
}

/// A Git project `/repo` (workspace `w`) whose issues are on GitHub, and a
/// folder `/notes` (workspace `n`) whose issues are memos.
fn backlog_runtime(gh: Arc<FakeGh>) -> (SharedRuntime, crate::links::worker::LinkWorker) {
    backlog_runtime_with(gh, |_| {})
}

/// `backlog_runtime`, with `prepare` run before the link worker opens its
/// store.
fn backlog_runtime_with(
    gh: Arc<FakeGh>,
    prepare: impl FnOnce(&mut Runtime),
) -> (SharedRuntime, crate::links::worker::LinkWorker) {
    let mut runtime = runtime();
    let mut project = workspace(
        "w",
        "Repo",
        "/repo",
        vec![checkout("w", "c", "/repo", None)],
    );
    project.is_git = true;
    project.home_issues = ProjectIssuesSnapshot {
        repository: Some("acme/project".into()),
        ..Default::default()
    };
    let notes = workspace(
        "n",
        "Notes",
        "/notes",
        vec![checkout("n", "d", "/notes", None)],
    );
    runtime.snapshot.navigator.workspaces = vec![project, notes];
    runtime.local_issues = Ok(Default::default());
    runtime.own_node = gh;
    prepare(&mut runtime);
    let shared = SharedRuntime::new(runtime);
    shared
        .lock()
        .unwrap()
        .install_worker_context(shared.weak(), ChangeNotifier::noop());
    let worker = super::super::links::spawn_worker(&shared, ChangeNotifier::noop())
        .expect("the link worker starts");
    (shared, worker)
}

fn send(shared: &SharedRuntime, kind: &str, payload: serde_json::Value) {
    let event =
        serde_json::json!({"schema_version": SCHEMA_VERSION, "kind": kind, "payload": payload});
    shared
        .lock()
        .unwrap()
        .dispatch_json(&serde_json::to_vec(&event).unwrap());
}

fn add(shared: &SharedRuntime, intent: &str, workspace: &str, title: &str) {
    send(
        shared,
        "backlog_add",
        serde_json::json!({"intent": intent, "workspace_id": workspace, "title": title}),
    );
}

fn phases(runtime: &Runtime) -> Vec<(String, BacklogPhase, Option<BacklogFailure>)> {
    let mut rows: Vec<_> = runtime
        .snapshot
        .issue_work
        .backlog
        .iter()
        .map(|row| (row.intent.clone(), row.phase, row.failure))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

fn row_key(runtime: &Runtime, intent: &str) -> Option<String> {
    runtime
        .snapshot
        .issue_work
        .backlog
        .iter()
        .find(|row| row.intent == intent)?
        .task_key
        .clone()
}

#[test]
fn a_github_item_is_one_title_only_create_and_a_repeated_send_makes_no_second() {
    let gh = FakeGh::new();
    let (shared, _worker) = backlog_runtime(gh.clone());
    add(&shared, "i-1", "w", "Flaky e2e on Windows");
    add(&shared, "i-1", "w", "Flaky e2e on Windows");
    wait(&shared, "the issue", |runtime| {
        row_key(runtime, "i-1").is_some()
    });
    let runtime = shared.lock().unwrap();
    assert_eq!(
        row_key(&runtime, "i-1").as_deref(),
        Some("github:acme/project#41")
    );
    assert_eq!(
        gh.calls("issue create"),
        ["issue create --title Flaky e2e on Windows --body "]
    );
    let issues = &runtime.github.projects;
    assert!(
        issues.is_empty() || issues[0].issues.issues[0].reference.number == 41,
        "the new issue shows before the next read"
    );
}

#[test]
fn one_create_runs_per_device_with_eight_waiting_and_the_next_is_refused_as_full() {
    let gh = FakeGh::new();
    gh.gate(false);
    let (shared, _worker) = backlog_runtime(gh.clone());
    for index in 0..10 {
        add(
            &shared,
            &format!("i-{index:02}"),
            "w",
            &format!("Item {index}"),
        );
    }
    wait(&shared, "the first create to reach gh", |_| {
        !gh.calls("issue create").is_empty()
    });
    {
        let runtime = shared.lock().unwrap();
        let rows = phases(&runtime);
        assert_eq!(rows[0].1, BacklogPhase::Creating);
        assert!(rows[1..9].iter().all(|row| row.1 == BacklogPhase::Waiting));
        assert_eq!(
            (rows[9].1, rows[9].2),
            (BacklogPhase::Failed, Some(BacklogFailure::QueueFull))
        );
    }
    gh.gate(true);
    wait(&shared, "the queue to drain", |runtime| {
        phases(runtime)[..9]
            .iter()
            .all(|row| row.1 == BacklogPhase::Created)
    });
    assert_eq!(gh.calls("issue create").len(), 9, "one create per item");

    send(
        &shared,
        "backlog_retry",
        serde_json::json!({"intent": "i-09"}),
    );
    wait(&shared, "the refused item", |runtime| {
        row_key(runtime, "i-09").is_some()
    });
    assert_eq!(
        row_key(&shared.lock().unwrap(), "i-09").as_deref(),
        Some("github:acme/project#50")
    );
}

#[test]
fn a_retry_after_a_restart_finds_the_issue_the_unconfirmed_create_made() {
    let gh = FakeGh::new();
    *gh.create_prints.lock().unwrap() = Some("Something went wrong".into());
    let (shared, _worker) = backlog_runtime(gh.clone());
    add(&shared, "i-1", "w", "Release notes");
    wait(&shared, "the unconfirmed create", |runtime| {
        phases(runtime).first().map(|row| (row.1, row.2))
            == Some((BacklogPhase::Failed, Some(BacklogFailure::Unconfirmed)))
    });
    // GitHub made #41 although gh's answer did not say so.
    gh.reconcile
        .lock()
        .unwrap()
        .push((41, "Release notes".into()));

    // A restart forgets the row; the ledger brings it back.
    {
        let mut runtime = shared.lock().unwrap();
        runtime.snapshot.issue_work.backlog.clear();
        runtime.backlog_work = Default::default();
        runtime.restore_pending_backlog();
    }
    wait(&shared, "the restored row", |runtime| {
        phases(runtime).first().map(|row| (row.1, row.2))
            == Some((BacklogPhase::Failed, Some(BacklogFailure::Unconfirmed)))
    });
    send(
        &shared,
        "backlog_retry",
        serde_json::json!({"intent": "i-1"}),
    );
    wait(&shared, "the retried item", |runtime| {
        row_key(runtime, "i-1").is_some()
    });
    assert_eq!(
        row_key(&shared.lock().unwrap(), "i-1").as_deref(),
        Some("github:acme/project#41")
    );
    assert_eq!(gh.calls("issue create").len(), 1, "no second issue");
}

#[test]
fn undo_deletes_a_memo_and_closes_a_github_issue() {
    let gh = FakeGh::new();
    let (shared, _worker) = backlog_runtime(gh.clone());
    add(&shared, "m-1", "n", "Buy a domain");
    let memo = row_key(&shared.lock().unwrap(), "m-1").expect("a memo is made at once");
    assert_eq!(memo, crate::tasks::local_key("/notes", 1));
    send(
        &shared,
        "backlog_undo",
        serde_json::json!({"workspace_id": "n", "task_key": memo}),
    );
    {
        let runtime = shared.lock().unwrap();
        let store = runtime.local_issues.as_ref().unwrap();
        assert!(store.issue(None, "/notes", 1).is_none());
        assert_eq!(
            runtime.snapshot.issue_work.undo.as_ref().unwrap().phase,
            "done"
        );
        assert!(row_key(&runtime, "m-1").is_none());
    }

    add(&shared, "i-1", "w", "Ship it");
    wait(&shared, "the issue", |runtime| {
        row_key(runtime, "i-1").is_some()
    });
    send(
        &shared,
        "backlog_undo",
        serde_json::json!({"workspace_id": "w", "task_key": "github:acme/project#41"}),
    );
    wait(&shared, "the close", |runtime| {
        runtime
            .snapshot
            .issue_work
            .undo
            .as_ref()
            .is_some_and(|undo| undo.phase == "done")
    });
    assert_eq!(
        gh.calls("issue close"),
        ["issue close 41 --repo acme/project"]
    );
}

#[test]
fn a_started_item_cannot_be_undone() {
    let gh = FakeGh::new();
    let (shared, _worker) = backlog_runtime(gh);
    add(&shared, "m-1", "n", "Started already");
    let memo = row_key(&shared.lock().unwrap(), "m-1").unwrap();
    shared.lock().unwrap().snapshot.navigator.workspaces[1].checkouts[0].task_key =
        Some(memo.clone());
    send(
        &shared,
        "backlog_undo",
        serde_json::json!({"workspace_id": "n", "task_key": memo}),
    );
    let runtime = shared.lock().unwrap();
    assert_eq!(
        runtime.snapshot.status.last_error.as_ref().unwrap().kind,
        "backlog.undo_started"
    );
    assert!(
        runtime
            .local_issues
            .as_ref()
            .unwrap()
            .issue(None, "/notes", 1)
            .is_some()
    );
}

fn failure(runtime: &Runtime, intent: &str) -> Option<(BacklogPhase, Option<BacklogFailure>)> {
    phases(runtime)
        .into_iter()
        .find(|row| row.0 == intent)
        .map(|row| (row.1, row.2))
}

#[test]
fn a_create_in_flight_when_the_repository_changes_is_not_published_and_its_retry_makes_none() {
    let gh = FakeGh::new();
    gh.gate(false);
    let (shared, _worker) = backlog_runtime(gh.clone());
    add(&shared, "i-1", "w", "Moved repository");
    wait(&shared, "the create to reach gh", |_| {
        !gh.calls("issue create").is_empty()
    });
    send(
        &shared,
        "project_settings_set",
        serde_json::json!({
            "device_id": crate::node::TEST_NODE, "root": "/repo", "request_id": "move",
            "patch": {"repository": {"mode": "repository", "repository": "acme/other"}},
        }),
    );
    let id = crate::project_settings::key(crate::node::TEST_NODE, "/repo");
    wait(&shared, "the saved connection", |runtime| {
        runtime
            .snapshot
            .ui_state
            .project_settings_writes
            .get(&id)
            .is_some_and(|write| write.state == crate::project_settings::WriteState::Saved)
    });
    gh.gate(true);
    wait(&shared, "the old create's answer", |runtime| {
        failure(runtime, "i-1").is_some_and(|row| row.0 == BacklogPhase::Failed)
    });
    {
        let runtime = shared.lock().unwrap();
        assert_eq!(
            failure(&runtime, "i-1"),
            Some((
                BacklogPhase::Failed,
                Some(BacklogFailure::RepositoryChanged)
            ))
        );
        assert!(row_key(&runtime, "i-1").is_none());
        assert!(
            runtime
                .github
                .projects
                .iter()
                .all(|project| project.issues.issues.is_empty()),
            "the old repository's issue is not shown under the new one"
        );
    }

    // The attempt is bound to the repository it was reserved for.
    send(
        &shared,
        "backlog_retry",
        serde_json::json!({"intent": "i-1"}),
    );
    wait(&shared, "the retry's answer", |runtime| {
        failure(runtime, "i-1").is_some_and(|row| row.0 == BacklogPhase::Failed)
    });
    assert_eq!(
        failure(&shared.lock().unwrap(), "i-1"),
        Some((
            BacklogPhase::Failed,
            Some(BacklogFailure::RepositoryChanged)
        ))
    );
    assert_eq!(
        gh.calls("issue create").len(),
        1,
        "no create for the new repository"
    );
}

#[test]
fn a_missed_line_whose_session_origin_repository_is_unknown_makes_no_issue() {
    use crate::links::ledger::SessionIdentity;
    let gh = FakeGh::new();
    let session = SessionIdentity {
        device_id: crate::node::TEST_NODE.into(),
        agent: "codex".into(),
        session_id: "s-1".into(),
    };
    let (shared, _worker) = backlog_runtime_with(gh.clone(), |runtime| {
        runtime.ai_settings = Some(hide_ai::settings::AiSettings {
            enabled: true,
            chosen: true,
            agent_summary: true,
            ..Default::default()
        });
        // The session is first read now, as old history: no repository is
        // proved for it.
        let store = hide_kit::layout::links_store(runtime.state_path.parent().unwrap());
        let (mut store, _) = crate::links::store::LinkStore::open(&store).unwrap();
        store
            .apply_answer(
                &session.device_id,
                &hide_session::links::ReadAnswer {
                    agent: hide_session::Agent::from_kind("codex").unwrap(),
                    path: "/fixture/s-1.jsonl".into(),
                    checkpoint: None,
                    has_more: false,
                    rescanned: true,
                    error: None,
                    facts: hide_session::links::LinkFacts {
                        session_id: Some(session.session_id.clone()),
                        cwd: Some("/repo".into()),
                        first_at_unix_ms: Some(1),
                        last_at_unix_ms: Some(1),
                        ..Default::default()
                    },
                },
                "history",
            )
            .unwrap();
        assert_eq!(store.ledger_record(&session).unwrap().repository, None);
    });
    for (intent, session_id) in [("m-1", "s-1"), ("m-2", "unknown")] {
        send(
            &shared,
            "backlog_add",
            serde_json::json!({
                "intent": intent, "workspace_id": "w", "title": "Missed follow-up",
                "missed": {"session": {"device_id": session.device_id, "agent": "codex", "session_id": session_id}, "id": "x"},
            }),
        );
        wait(&shared, "the missed line's answer", |runtime| {
            failure(runtime, intent).is_some_and(|row| row.0 == BacklogPhase::Failed)
        });
    }
    let runtime = shared.lock().unwrap();
    assert_eq!(
        failure(&runtime, "m-1").unwrap().1,
        Some(BacklogFailure::RepositoryChanged)
    );
    assert_eq!(
        failure(&runtime, "m-2").unwrap().1,
        Some(BacklogFailure::Refused)
    );
    assert!(gh.calls("issue create").is_empty());
}
