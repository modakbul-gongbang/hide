//! The link record through the runtime (PRD link-graph B12, B24, B25, B27,
//! D-45): which facts reach the worker, the panel read on its own revision,
//! and the resume start.

use super::*;
use crate::issues::{IssueReference, ProjectIssuesSnapshot};
use crate::links::{IssueSource, LinkTarget, SessionRole};
use crate::model::{PullRequestBadge, PullRequestChecks, PullRequestSnapshot};

const CREATED: u64 = 1_790_000_000_000;

fn pull_request(number: u32, branch: &str, closes: &[u32]) -> PullRequestSnapshot {
    PullRequestSnapshot {
        closing_issues: closes
            .iter()
            .map(|number| IssueReference {
                repository: "acme/project".into(),
                number: *number,
            })
            .collect(),
        title: format!("PR {number}"),
        checks: PullRequestChecks::Passing,
        number,
        head_branch: branch.into(),
        base_branch: "main".into(),
        url: format!("https://github.com/Acme/Project/pull/{number}"),
        badge: PullRequestBadge::Open,
        review: None,
        is_draft: false,
        merged_at_unix_ms: None,
        updated_at_unix_ms: Some(CREATED),
        created_at_unix_ms: Some(CREATED),
        closed_at_unix_ms: None,
        head_oid: None,
        cross_repository: false,
    }
}

/// A GitHub project `/repo` with a worktree `/repo/task` on `4-task`, whose
/// Hide issue link is local issue 4.
fn links_runtime(pull_requests: Vec<PullRequestSnapshot>, read: bool) -> Runtime {
    let mut runtime = runtime();
    let mut task = checkout("w", "c", "/repo/task", Some(pane("p", "/repo/task")));
    task.is_worktree = true;
    task.branch = Some("4-task".into());
    task.task_key = Some("local:/repo#4".into());
    let mut workspace = workspace("w", "Repo", "/repo", vec![task]);
    workspace.is_git = true;
    runtime.snapshot.navigator.workspaces = vec![workspace];
    runtime
        .github
        .projects
        .push(crate::model::GithubProjectSnapshot {
            root_path: "/repo".into(),
            issues: ProjectIssuesSnapshot {
                repository: Some("Acme/Project".into()),
                issues: Vec::new(),
                overflow: false,
                dependencies_failure: None,
                repository_id: Some("R_kgDO".into()),
            },
            status: crate::model::GithubStatusSnapshot {
                available: true,
                last_success_at_unix_ms: Some(1),
                ..Default::default()
            },
            pull_requests,
            pull_requests_read: read,
            issues_read: true,
        });
    runtime
}

#[test]
fn the_worker_is_handed_each_pull_request_with_its_issue_links_and_sources() {
    let runtime = links_runtime(
        vec![
            pull_request(7, "4-task", &[3]),
            pull_request(8, "other", &[]),
        ],
        true,
    );

    let projects = runtime.link_projects();
    assert_eq!(projects.len(), 1);
    let project = &projects[0];
    assert_eq!(
        project.key,
        hide_project::project_id(crate::node::TEST_NODE, Path::new("/repo"))
    );
    assert_eq!(project.repository.as_deref(), Some("acme/project"));
    assert_eq!(project.repository_id.as_deref(), Some("R_kgDO"));
    assert!(project.prs_read);
    let seven = project.prs.iter().find(|pr| pr.number == 7).unwrap();
    assert_eq!(seven.repository, "acme/project");
    assert_eq!(
        seven.issues,
        vec![
            ("github:acme/project#3".to_owned(), IssueSource::Closes),
            ("local:/repo#4".to_owned(), IssueSource::Hide),
        ]
    );
    assert!(seven.hide_issue_known);
    // No checkout holds `other`, so whether its Hide link was cleared is
    // unknown and nothing of that source may close (D-35).
    let eight = project.prs.iter().find(|pr| pr.number == 8).unwrap();
    assert!(!eight.hide_issue_known);
}

#[test]
fn a_failed_github_read_hands_the_worker_nothing_to_close() {
    let runtime = links_runtime(vec![pull_request(7, "4-task", &[3])], false);
    assert!(!runtime.link_projects()[0].prs_read);
}

/// A checkout whose HEAD Git finds in its base reads as merged only once the
/// record says the work done there landed: one with no commits of its own
/// stays a plain checkout in the main list, and new commits on it take the
/// record's answer away.
#[test]
fn a_checkout_in_its_base_is_landed_only_when_the_record_says_its_work_landed() {
    let mut runtime = links_runtime(Vec::new(), true);
    let in_base = |runtime: &mut Runtime, merged| {
        runtime.snapshot.navigator.workspaces[0].checkouts[0].worktree =
            Some(crate::model::WorktreeSnapshot {
                path: "/repo/task".into(),
                merged: Some(merged),
                ..Default::default()
            });
        runtime.apply_pull_requests();
        runtime.refresh_inactive_groups();
    };
    let shown = |runtime: &Runtime| {
        let workspace = &runtime.snapshot.navigator.workspaces[0];
        (
            workspace.checkouts[0].landed,
            workspace.inactive_checkouts.checkout_ids.clone(),
        )
    };
    in_base(&mut runtime, true);
    assert_eq!(shown(&runtime), (false, Vec::<String>::new()));

    let summary = |paths: &[&str]| {
        std::collections::BTreeMap::from([(
            "w".to_owned(),
            crate::links::ProjectLinkSummary {
                landed: paths.iter().map(|path| (*path).to_owned()).collect(),
                ..Default::default()
            },
        )])
    };
    assert!(runtime.ingest_link_summaries(summary(&["/repo/task"])));
    assert_eq!(shown(&runtime), (true, vec!["c".to_owned()]));
    let wire = serde_json::to_value(runtime.snapshot()).unwrap();
    assert_eq!(
        wire["navigator"]["workspaces"][0]["checkouts"][0]["landed"],
        true
    );
    assert!(
        wire["link_summaries"]["projects"]["w"]
            .get("landed")
            .is_none(),
        "the record's set stays in the core"
    );

    in_base(&mut runtime, false);
    assert_eq!(shown(&runtime), (false, Vec::<String>::new()));
}

fn claude_session(home: &Path, id: &str, printed_at: u64) {
    let dir = home.join(".claude/projects/-repo-task");
    std::fs::create_dir_all(&dir).unwrap();
    let at = |ms: u64| {
        jiff::Timestamp::from_millisecond(ms as i64)
            .unwrap()
            .to_string()
    };
    let lines = [
        serde_json::json!({
            "type": "user", "isSidechain": false, "uuid": "u1", "parentUuid": null,
            "message": {"role": "user", "content": "PR 올려 줘"}, "timestamp": at(printed_at - 60_000),
            "promptId": "p", "origin": {"kind": "human"}, "userType": "external",
            "entrypoint": "cli", "cwd": "/repo/task", "sessionId": id, "gitBranch": "4-task",
        }),
        serde_json::json!({
            "type": "pr-link", "sessionId": id, "prNumber": 7, "prRepository": "acme/project",
            "prUrl": "https://github.com/acme/project/pull/7", "timestamp": at(printed_at),
        }),
    ];
    let text = lines.map(|line| line.to_string()).join("\n") + "\n";
    std::fs::write(dir.join(format!("{id}.jsonl")), text).unwrap();
}

fn open_event(target: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION, "kind": "links_open",
        "payload": {"workspace_id": "w", "target": target}
    }))
    .unwrap()
}

#[test]
fn opening_a_pull_request_reads_its_sessions_on_the_panels_own_revision() {
    let home = scratch_dir("herdr-core-links-home-");
    claude_session(home.path(), "s-maker", CREATED + 1_000);
    let mut runtime = links_runtime(vec![pull_request(7, "4-task", &[3])], true);
    runtime.own_node = Arc::new(hide_node::Local::new(Some(home.path().to_path_buf())));
    let shared = Arc::new(Mutex::new(runtime));
    let worker = super::super::links::spawn_worker(&shared, ChangeNotifier::noop())
        .expect("the link worker starts");

    assert!(
        shared
            .lock()
            .unwrap()
            .dispatch_json(&open_event(serde_json::json!({"kind": "pr", "number": 7})))
    );
    {
        let runtime = shared.lock().unwrap();
        let panel = runtime.snapshot.link_panel.as_ref().unwrap();
        assert!(panel.loading, "B24: the first read shows its spinner");
        assert_eq!(panel.target, LinkTarget::Pr { number: 7 });
    }
    wait(&shared, "the maker's line", |runtime| {
        runtime
            .snapshot
            .link_panel
            .as_ref()
            .is_some_and(|panel| !panel.loading && panel.sessions.len() == 1)
    });
    let mut runtime = shared.lock().unwrap();
    let panel = runtime.snapshot.link_panel.clone().unwrap();
    let line = &panel.sessions[0];
    assert_eq!(
        (line.id.as_str(), line.role),
        ("s-maker", SessionRole::Created)
    );
    assert_eq!(line.request.as_deref(), Some("PR 올려 줘"));
    let pr = panel.pr.unwrap();
    assert_eq!(pr.branch, "4-task");
    assert_eq!(pr.issues.len(), 2);

    let first = runtime.snapshot_delta_payload(0, 0);
    assert!(first.link_panel.is_some());
    let again = runtime.snapshot_delta_payload(first.revision, 0);
    assert!(again.link_panel.is_none(), "an unchanged panel is not sent");
    drop(runtime);
    wait(&shared, "the session chip", |runtime| {
        runtime
            .snapshot
            .link_summaries
            .as_ref()
            .and_then(|summaries| summaries.projects.get("w"))
            .is_some_and(|summary| summary.prs.get(&7) == Some(&1))
    });
    drop(worker);
}

#[test]
fn an_answer_for_a_superseded_read_is_dropped_and_a_failure_keeps_the_lines() {
    let mut runtime = links_runtime(vec![pull_request(7, "4-task", &[])], true);
    // No worker: the read fails in place with a code (B25).
    assert!(runtime.dispatch_json(&open_event(serde_json::json!({"kind": "pr", "number": 7}))));
    let panel = runtime.snapshot.link_panel.clone().unwrap();
    assert_eq!(panel.failure.as_deref(), Some("links_worker_unavailable"));
    assert!(!panel.loading);

    let generation = runtime.links_generation();
    let empty = || {
        Ok(crate::links::worker::PanelAnswer::Issue(
            crate::links::store::IssueLinks::default(),
        ))
    };
    assert!(!runtime.ingest_link_panel(generation - 1, empty()));
    let mut held = runtime.snapshot.link_panel.clone().unwrap();
    held.sessions = vec![crate::links::LinkedSession {
        agent: "claude".into(),
        id: "s1".into(),
        ids: vec!["s1".into()],
        device_id: crate::node::TEST_NODE.into(),
        role: SessionRole::Worked,
        pr: 7,
        request: None,
        started_at_unix_ms: None,
        ended_at_unix_ms: None,
        path: None,
        cwd: None,
        file: crate::links::FileState::Unknown,
        parent: None,
        on_branch: false,
    }];
    runtime.snapshot.link_panel = Some(held);
    assert!(runtime.ingest_link_panel(generation, Err("links_store_busy".into())));
    let panel = runtime.snapshot.link_panel.clone().unwrap();
    assert_eq!(panel.failure.as_deref(), Some("links_store_busy"));
    assert_eq!(panel.sessions.len(), 1);
}

fn resume_event(provider: &str, session: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION, "kind": "agent_start_in_checkout",
        "payload": {"checkout_path": "/repo/task", "provider": provider,
                    "resume_session_id": session, "request_id": "r"}
    }))
    .unwrap()
}

/// A retired external server stays usable, but cannot borrow the current
/// file node's proof to create or launch a queued archived conversation.
#[test]
#[cfg(unix)]
fn a_pi_archive_worker_refuses_retired_control_before_creation_or_start() {
    for case in ["local-before", "remote-before", "local-after-tab"] {
        let home = tempfile::tempdir().unwrap();
        let cwd = home.path().canonicalize().unwrap().display().to_string();
        let native_folder = home.path().join(".pi/agent/sessions").join(format!(
            "--{}--",
            cwd.trim_start_matches('/').replace(['/', '\\', ':'], "-")
        ));
        std::fs::create_dir_all(&native_folder).unwrap();
        let native = native_folder.join("selected.jsonl");
        let bytes = format!(
            "{}\n",
            serde_json::json!({"type":"session", "version":3, "id":"native-pi", "cwd":cwd})
        );
        std::fs::write(&native, &bytes).unwrap();
        let (mut runtime, _) = live_tab_order_runtime(&cwd);
        runtime.own_node = Arc::new(hide_node::Local::new(Some(home.path().to_path_buf())));
        runtime.live.as_mut().unwrap().node = runtime.own_node();
        let id = runtime
            .begin_task_operation(
                "agent_start",
                Some(cwd.clone()),
                None,
                None,
                Some("pi".into()),
            )
            .unwrap();
        runtime.set_task_agent_launch(id, None, vec!["--session".into(), "native-pi".into()]);
        let reference = crate::sidebar::SessionAgentSessionPayload {
            kind: "path".into(),
            value: native.display().to_string(),
        };
        runtime.task_agent_launch.as_mut().unwrap().resume_reference = Some(reference.clone());
        let shared = Arc::new(Mutex::new(runtime));
        let replacement = FakeHerdr::start("pi-resume-current", |method, _| {
            panic!("unexpected current-server request: {method}")
        });
        let replacement_connector: Arc<dyn hide_herdr_client::ApiConnector> =
            Arc::new(replacement.connector());
        let changed_runtime = Arc::downgrade(&shared);
        let changed_connector = Arc::clone(&replacement_connector);
        let old = FakeHerdr::start("pi-resume-retired", move |method, _| {
            assert_eq!(
                case, "local-after-tab",
                "no retired effect is permitted before creation"
            );
            assert_eq!(method, "tab.create");
            let runtime = changed_runtime.upgrade().unwrap();
            let mut runtime = runtime.lock().unwrap();
            let mut next = runtime.live.as_ref().unwrap().clone();
            next.api_connector = Arc::clone(&changed_connector);
            runtime.set_live(next);
            serde_json::json!({"type":"tab_created", "tab":{
                "tab_id":"w-order:t3","workspace_id":"w-order","number":3,"label":"resume","focused":true,"pane_count":1,"agent_status":"idle"
            },"root_pane":{"pane_id":"wake-pane","terminal_id":"wake-terminal","workspace_id":"w-order","tab_id":"w-order:t3","focused":true,"agent_status":"idle","revision":0}})
        });
        let target = {
            let mut runtime = shared.lock().unwrap();
            let mut context = runtime.live.as_ref().unwrap().clone();
            context.runtime = Arc::downgrade(&shared);
            context.api_connector = Arc::new(old.connector());
            runtime.set_live(context.clone());
            if case == "remote-before" {
                runtime.snapshot.task_operation.as_mut().unwrap().device_id = Some("remote".into());
                let remote = live::RemoteControlContext::new(
                    "remote",
                    Arc::clone(&context.api_connector),
                    Arc::downgrade(&shared),
                    crate::handle::ChangeNotifier::noop(),
                );
                runtime.install_remote_control(remote.clone());
                let target = live::TabTarget::device(&remote);
                runtime.install_remote_control(live::RemoteControlContext::new(
                    "remote",
                    Arc::clone(&replacement_connector),
                    Arc::downgrade(&shared),
                    crate::handle::ChangeNotifier::noop(),
                ));
                target
            } else {
                let target = live::TabTarget::local(&context, runtime.live_generation);
                if case == "local-before" {
                    context.api_connector = Arc::clone(&replacement_connector);
                    runtime.set_live(context);
                }
                target
            }
        };
        live::spawn_checkout_tab_create(
            target,
            live::CheckoutTabRequest {
                id,
                checkout_path: cwd.clone(),
                label: "resume".into(),
                host: crate::checkout_owner::TabHost::Workspace("w-order".into()),
                resume_scope: Some(hide_session::SessionReadScope {
                    id: "native-pi".into(),
                    cwd,
                }),
                resume_reference: Some(reference),
            },
        )
        .unwrap();
        wait_for("retired archive control refusal", || {
            let runtime = shared.lock().unwrap();
            let operation = runtime.snapshot.task_operation.as_ref().unwrap();
            operation.phase == "failed" || operation.agent_phase.as_deref() == Some("failed")
        });
        let runtime = shared.lock().unwrap();
        let operation = runtime.snapshot.task_operation.as_ref().unwrap();
        let reason = operation
            .agent_message
            .as_deref()
            .or(operation.message.as_deref())
            .unwrap();
        assert_eq!(reason, "session_resume_control_changed", "{case}");
        assert_eq!(
            old.methods(),
            if case == "local-after-tab" {
                vec!["tab.create"]
            } else {
                vec![]
            }
        );
        assert!(replacement.methods().is_empty());
        assert_eq!(std::fs::read_to_string(&native).unwrap(), bytes);
    }
}

#[test]
fn resuming_starts_the_providers_resume_and_refuses_what_cannot_resume() {
    let mut runtime = runtime();
    runtime.ingest_session(Ok(context_payload()));
    let path = runtime.snapshot.navigator.workspaces[0].checkouts[0]
        .path
        .clone();
    let event = |provider: &str, session: &str| {
        let mut value: serde_json::Value =
            serde_json::from_slice(&resume_event(provider, session)).unwrap();
        value["payload"]["checkout_path"] = path.clone().into();
        if provider == "pi" {
            // B5/B8 admission retains the catalog's selected file. The
            // owning worker proves this expectation before any effect.
            value["payload"]["resume_session_path"] = "/fixture/selected-pi.jsonl".into();
        }
        serde_json::to_vec(&value).unwrap()
    };

    assert!(runtime.dispatch_json(&event("codex", "019a-session")));
    let operation = runtime.snapshot.task_operation.clone().expect("operation");
    assert_eq!(operation.agent_kind.as_deref(), Some("codex"));
    assert_eq!(
        runtime
            .task_agent_launch
            .as_ref()
            .map(|launch| launch.args.clone()),
        Some(vec!["resume".to_owned(), "019a-session".to_owned()])
    );

    runtime.snapshot.task_operation = None;
    assert!(runtime.dispatch_json(&event("pi", "native.one")));
    assert_eq!(
        runtime
            .task_agent_launch
            .as_ref()
            .map(|launch| launch.args.clone()),
        Some(vec!["--session".to_owned(), "native.one".to_owned()])
    );

    // Starts do not grant resume support before each complete reader
    // slice lands; an id that is not one token never reaches a command line.
    for (provider, session, kind) in [
        ("opencode", "ses_1", "agent_start.invalid_resume"),
        ("pi", "native.jsonl", "agent_start.invalid_resume"),
        ("pi", "../native", "agent_start.invalid_resume"),
        ("omp", "native-one", "agent_start.invalid_resume"),
        ("grok", "native-one", "agent_start.invalid_resume"),
        ("cursor", "native-one", "agent_start.invalid_resume"),
        ("unknown", "native-one", "agent_start.unknown_provider"),
        ("claude", "a b", "agent_start.invalid_resume"),
        ("claude", "--dangerous", "agent_start.invalid_resume"),
    ] {
        runtime.snapshot.task_operation = None;
        assert!(runtime.dispatch_json(&event(provider, session)));
        assert_eq!(
            runtime
                .snapshot
                .status
                .last_error
                .as_ref()
                .map(|error| error.kind.as_str()),
            Some(kind),
            "{provider} {session}"
        );
        assert!(runtime.snapshot.task_operation.is_none());
    }
}
