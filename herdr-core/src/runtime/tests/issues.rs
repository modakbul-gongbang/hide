use super::*;
use crate::issues::{IssueReference, IssueSnapshot, ProjectIssuesSnapshot};

fn issue(number: u32) -> IssueSnapshot {
    IssueSnapshot {
        reference: IssueReference {
            repository: "acme/project".into(),
            number,
        },
        title: format!("Task {number}"),
        url: format!("https://github.com/acme/project/issues/{number}"),
        state: "OPEN".into(),
        project_status: None,
        updated_at_unix_ms: Some(1),
        blocked_by: Vec::new(),
    }
}
fn issue_runtime() -> Runtime {
    let mut runtime = runtime();
    let mut checkout = checkout("w", "c", "/repo/task", Some(pane("p", "/repo/task")));
    checkout.is_worktree = true;
    checkout.branch = Some("4-task".into());
    checkout.branch_issue = Some("acme/project#2".into());
    let mut workspace = workspace("w", "Repo", "/repo", vec![checkout]);
    workspace.is_git = true;
    workspace.home_issues = ProjectIssuesSnapshot {
        repository: Some("acme/project".into()),
        issues: (1..=4).map(issue).collect(),
        overflow: false,
        dependencies_failure: None,
    };
    runtime.snapshot.navigator.workspaces = vec![workspace];
    runtime
}
#[test]
fn issue_precedence_and_refresh_follow_identity_transitions() {
    let mut runtime = issue_runtime();
    runtime
        .issue_tokens
        .panes
        .insert("p".into(), "acme/project#1".into());
    assert!(runtime.sync_issues());
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0].checkouts[0]
            .issue
            .as_ref()
            .unwrap()
            .issue
            .reference
            .number,
        1
    );
    let generations = runtime.github_generations.clone();
    assert!(!runtime.sync_issues());
    assert_eq!(
        runtime.github_generations, generations,
        "idle projection never polls GitHub"
    );
    runtime.issue_tokens.panes.clear();
    assert!(runtime.sync_issues());
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0].checkouts[0]
            .issue
            .as_ref()
            .unwrap()
            .issue
            .reference
            .number,
        2
    );
    runtime.snapshot.navigator.workspaces[0].checkouts[0].branch_issue = None;
    assert!(runtime.sync_issues());
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0].checkouts[0]
            .issue
            .as_ref()
            .unwrap()
            .issue
            .reference
            .number,
        4
    );
}
#[test]
fn missing_issue_details_never_fabricate_a_chip() {
    let mut runtime = issue_runtime();
    runtime.snapshot.navigator.workspaces[0]
        .home_issues
        .issues
        .clear();
    runtime.sync_issues();
    assert!(
        runtime.snapshot.navigator.workspaces[0].checkouts[0]
            .issue
            .is_none()
    );
    assert_eq!(runtime.issue_candidates["c"].reference.number, 2);
}
#[test]
fn successful_manual_issue_is_immediate_even_before_first_project_read() {
    let mut runtime = issue_runtime();
    runtime.issue_write_pending = Some((7, "c".into(), "acme/project#3".into()));
    let request = crate::live::PurposeTaskRequest {
        id: 7,
        checkout_id: "c".into(),
        repository_root: "/repo".into(),
        branch: Some("4-task".into()),
        session_workspace_id: None,
        purpose: "acme/project#3".into(),
    };
    runtime.ingest_issue_operation_result(&request, Ok(Some(issue(3))));
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0].checkouts[0]
            .issue
            .as_ref()
            .unwrap()
            .issue
            .reference
            .number,
        3
    );
    runtime.sync_issues();
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0].checkouts[0]
            .issue
            .as_ref()
            .unwrap()
            .issue
            .reference
            .number,
        3
    );
    assert!(runtime.snapshot.git_worktrees_loading);
}

fn manual_issue_runtime() -> Runtime {
    let mut runtime = issue_runtime();
    runtime.snapshot.navigator.workspaces[0].session_workspace_ids = vec!["w".into()];
    runtime.last_session_spaces = vec![workspace::SessionSpace {
        id: "w".into(),
        label: "Repo".into(),
        cwds: vec!["/repo/task".into()],
        purpose: None,
    }];
    runtime
        .issue_tokens
        .workspaces
        .insert("w".into(), "acme/project#1".into());
    runtime
        .issue_tokens
        .panes
        .insert("p".into(), "acme/project#2".into());
    runtime.snapshot.navigator.workspaces[0].checkouts[0].branch_issue =
        Some("acme/project#1".into());
    runtime.sync_issues();
    runtime
}
fn manual_request(number: u32) -> crate::live::PurposeTaskRequest {
    crate::live::PurposeTaskRequest {
        id: 7,
        checkout_id: "c".into(),
        repository_root: "/repo".into(),
        branch: Some("4-task".into()),
        session_workspace_id: Some("w".into()),
        purpose: format!("acme/project#{number}"),
    }
}
fn linked_number(runtime: &Runtime) -> u32 {
    runtime.snapshot.navigator.workspaces[0].checkouts[0]
        .issue
        .as_ref()
        .unwrap()
        .issue
        .reference
        .number
}
#[test]
fn issue_validation_failure_keeps_the_previously_saved_manual_override() {
    let mut runtime = manual_issue_runtime();
    assert_eq!(linked_number(&runtime), 1);
    let request = manual_request(1);
    runtime.issue_write_pending = Some((7, "c".into(), request.purpose.clone()));
    runtime.ingest_issue_operation_result(
        &request,
        Err(crate::live::IssueWriteFailure::unchanged(
            "read failed before mutation".into(),
        )),
    );
    runtime.sync_issues();
    assert_eq!(linked_number(&runtime), 1);
    assert!(runtime.unconfirmed_issue_tokens.is_empty());
}
#[test]
fn issue_uncertain_write_is_suppressed_even_when_metadata_arrives_later() {
    let mut runtime = manual_issue_runtime();
    let request = manual_request(3);
    runtime.issue_write_pending = Some((7, "c".into(), request.purpose.clone()));
    runtime.ingest_issue_operation_result(
        &request,
        Err(crate::live::IssueWriteFailure {
            detail: "rollback uncertain".into(),
            unconfirmed_token: true,
        }),
    );
    runtime.sync_issues();
    assert_eq!(linked_number(&runtime), 1);
    runtime
        .issue_tokens
        .workspaces
        .insert("w".into(), "acme/project#3".into());
    runtime.sync_issues();
    assert_ne!(linked_number(&runtime), 3);
    runtime
        .issue_tokens
        .workspaces
        .insert("w".into(), "acme/project#1".into());
    runtime.sync_issues();
    assert_eq!(linked_number(&runtime), 1);
    assert!(runtime.unconfirmed_issue_tokens.is_empty());
}

#[test]
fn a_linked_checkout_names_its_task_in_the_projects_task_list() {
    let mut runtime = issue_runtime();
    runtime.snapshot.navigator.workspaces[0].checkouts[0]
        .github
        .last_success_at_unix_ms = Some(1);
    runtime.snapshot.navigator.workspaces[0].checkouts[0]
        .github
        .available = true;
    let reference = |number| IssueReference {
        repository: "acme/project".into(),
        number,
    };
    // One pull request closing the linked issue and another one.
    runtime.snapshot.navigator.workspaces[0].checkouts[0].pull_request =
        Some(crate::model::PullRequestSnapshot {
            closing_issues: vec![reference(2), reference(3), reference(99)],
            title: "Fix".into(),
            checks: crate::model::PullRequestChecks::Unknown,
            number: 7,
            head_branch: "4-task".into(),
            base_branch: "main".into(),
            url: "https://example.invalid/pull/7".into(),
            badge: crate::model::PullRequestBadge::Open,
            review: None,
            is_draft: false,
            merged_at_unix_ms: None,
            updated_at_unix_ms: None,
        });
    runtime.sync_issues();
    assert!(runtime.sync_tasks());
    let workspace = &runtime.snapshot.navigator.workspaces[0];
    // The linked task is not repeated and an issue outside the list is left out.
    assert_eq!(
        workspace.checkouts[0].closes_task_keys,
        vec!["github:acme/project#3".to_owned()]
    );
    assert_eq!(workspace.tasks.tasks.len(), 4);
    assert_eq!(
        workspace.checkouts[0].task_key.as_deref(),
        Some("github:acme/project#2")
    );
    assert!(
        workspace
            .tasks
            .tasks
            .iter()
            .any(|task| task.key == "github:acme/project#2")
    );
    assert!(
        !runtime.sync_tasks(),
        "an unchanged projection publishes nothing"
    );
}

#[test]
fn a_pass_that_cannot_read_dependencies_keeps_the_blockers_read_before() {
    use crate::model::{GithubProjectSnapshot, GithubSnapshot, GithubStatusSnapshot};
    let mut runtime = issue_runtime();
    let answer = |blocked_by: Vec<IssueReference>, failure: Option<&str>| {
        let mut blocked = issue(2);
        blocked.blocked_by = blocked_by;
        GithubSnapshot {
            projects: vec![GithubProjectSnapshot {
                issues: ProjectIssuesSnapshot {
                    repository: Some("acme/project".into()),
                    issues: vec![issue(1), blocked],
                    overflow: false,
                    dependencies_failure: failure.map(str::to_owned),
                },
                root_path: "/repo".into(),
                status: GithubStatusSnapshot {
                    available: true,
                    last_success_at_unix_ms: Some(1),
                    ..Default::default()
                },
                pull_requests: Vec::new(),
                pull_requests_read: true,
                issues_read: true,
            }],
        }
    };
    let blocker = IssueReference {
        repository: "acme/project".into(),
        number: 1,
    };
    assert!(runtime.ingest_github(answer(vec![blocker.clone()], None)));
    let blockers = |runtime: &Runtime| {
        runtime.snapshot.navigator.workspaces[0]
            .tasks
            .tasks
            .iter()
            .find(|task| task.key == "github:acme/project#2")
            .map(|task| {
                task.blocked_by
                    .iter()
                    .map(|b| b.key.clone())
                    .collect::<Vec<_>>()
            })
    };
    assert_eq!(
        blockers(&runtime),
        Some(vec!["github:acme/project#1".to_owned()])
    );
    runtime.ingest_github(answer(Vec::new(), Some("rate limited")));
    assert_eq!(
        blockers(&runtime),
        Some(vec!["github:acme/project#1".to_owned()]),
        "the failed pass keeps the earlier blockers"
    );
    let source = runtime.snapshot.navigator.workspaces[0]
        .tasks
        .source
        .clone()
        .unwrap();
    assert_eq!(
        source.failure.as_deref(),
        Some("issue dependencies: rate limited")
    );
}

fn event(kind: &str, payload: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"schema_version": 2, "kind": kind, "payload": payload}))
        .unwrap()
}

/// A Git project whose `gh` says it has no GitHub remote reads Local issues
/// by default; a created issue is a task at once, and its checkout links to
/// it through the number its branch starts with.
#[test]
fn a_project_without_github_reads_local_issues_and_links_by_branch_number() {
    let mut runtime = issue_runtime();
    {
        let workspace = &mut runtime.snapshot.navigator.workspaces[0];
        workspace.checkouts[0].branch_issue = None;
        workspace.checkouts[0].branch = Some("1-local-work".into());
    }
    // What the GitHub reader answers for a repository with no GitHub remote.
    runtime
        .github
        .projects
        .push(crate::model::GithubProjectSnapshot {
            root_path: "/repo".into(),
            status: crate::model::GithubStatusSnapshot {
                unavailable_reason: Some("none of the git remotes point to GitHub".into()),
                failure_category: Some("no GitHub remote".into()),
                ..Default::default()
            },
            ..Default::default()
        });
    runtime.apply_pull_requests();
    let source = runtime.snapshot.navigator.workspaces[0]
        .tasks
        .source
        .clone()
        .unwrap();
    assert_eq!((source.kind.as_str(), source.chosen), ("local", false));

    assert!(runtime.dispatch_json(&event(
        "issue_create",
        serde_json::json!({"workspace_id": "w", "title": "로컬 첫 이슈", "body": "본문"}),
    )));
    let create = runtime.snapshot().issue_work.create.clone().unwrap();
    assert_eq!(create.phase, "ready");
    assert_eq!(create.task_key.as_deref(), Some("local:/repo#1"));
    let workspace = &runtime.snapshot().navigator.workspaces[0];
    assert_eq!(workspace.tasks.tasks[0].id.as_deref(), Some("L-1"));
    assert_eq!(
        workspace.checkouts[0].task_key.as_deref(),
        Some("local:/repo#1")
    );

    // The Start dialog reads the body from the store, with no worker.
    assert!(runtime.dispatch_json(&event(
        "issue_detail_request",
        serde_json::json!({"workspace_id": "w", "task_key": "local:/repo#1"}),
    )));
    let detail = runtime.snapshot().issue_work.detail.clone().unwrap();
    assert_eq!(
        (detail.phase.as_str(), detail.body.as_deref()),
        ("ready", Some("본문"))
    );

    // Closing it keeps it as a closed task.
    assert!(runtime.dispatch_json(&event(
        "issue_set_open",
        serde_json::json!({"task_key": "local:/repo#1", "open": false}),
    )));
    assert!(!runtime.snapshot().navigator.workspaces[0].tasks.tasks[0].open);
}

/// Settings › Issues chooses a project's source and `auto` gives it back;
/// the choice is part of the saved UI state.
#[test]
fn a_chosen_source_wins_over_the_default_and_auto_returns_it() {
    let mut runtime = issue_runtime();
    runtime.apply_pull_requests();
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0]
            .tasks
            .source
            .as_ref()
            .unwrap()
            .kind,
        "github"
    );
    assert!(runtime.dispatch_json(&event(
        "issue_source_set",
        serde_json::json!({"project_path": "/repo", "source": "local"}),
    )));
    let workspace = &runtime.snapshot().navigator.workspaces[0];
    let source = workspace.tasks.source.as_ref().unwrap();
    assert_eq!((source.kind.as_str(), source.chosen), ("local", true));
    // A GitHub link is not a local one: `acme/project#2` names no local issue.
    assert_eq!(workspace.checkouts[0].task_key, None);
    assert!(workspace.checkouts[0].issue.is_none());
    assert_eq!(
        runtime
            .snapshot()
            .ui_state
            .project_issue_sources
            .get("/repo")
            .map(String::as_str),
        Some("local")
    );
    assert!(runtime.dispatch_json(&event(
        "issue_source_set",
        serde_json::json!({"project_path": "/repo", "source": "auto"}),
    )));
    assert_eq!(
        runtime.snapshot.navigator.workspaces[0]
            .tasks
            .source
            .as_ref()
            .unwrap()
            .kind,
        "github"
    );
    // An unknown source is refused, not stored.
    assert!(runtime.dispatch_json(&event(
        "issue_source_set",
        serde_json::json!({"project_path": "/repo", "source": "jira"}),
    )));
    assert!(runtime.snapshot().ui_state.project_issue_sources.is_empty());
}

/// A title the store would refuse fails the create slot with the reason; a
/// GitHub create without a worker says so instead of hanging in `working`.
#[test]
fn a_refused_issue_create_settles_its_slot_with_the_reason() {
    let mut runtime = issue_runtime();
    runtime.apply_pull_requests();
    assert!(runtime.dispatch_json(&event(
        "issue_create",
        serde_json::json!({"workspace_id": "w", "title": "   "}),
    )));
    assert_eq!(
        runtime.snapshot().issue_work.create.as_ref().unwrap().phase,
        "failed"
    );
    assert!(runtime.dispatch_json(&event(
        "issue_create",
        serde_json::json!({"workspace_id": "w", "title": "GitHub issue"}),
    )));
    let create = runtime.snapshot().issue_work.create.clone().unwrap();
    assert_eq!(create.phase, "failed");
    assert!(create.message.unwrap().contains("No worker"));
}

/// Issue settings change only the fields sent; the agent a start picks is
/// no longer one of them (PRD home-device-rail D-18), so naming it is refused.
#[test]
fn issue_settings_change_only_what_was_sent() {
    let mut runtime = issue_runtime();
    assert!(runtime.dispatch_json(&event(
        "issue_settings_set",
        serde_json::json!({"ai_worktree_name": false}),
    )));
    let settings = runtime.snapshot().ui_state.issue_settings.clone();
    assert!(!settings.ai_worktree_name);
    assert!(settings.closes_instruction);
    // With AI naming off a suggestion is not even asked for.
    assert!(!runtime.dispatch_json(&event(
        "worktree_name_suggest",
        serde_json::json!({"request_id": "r1", "prefix": "1-", "title": "t"}),
    )));
    assert!(runtime.snapshot().issue_work.name.is_none());
    assert!(runtime.dispatch_json(&event(
        "issue_settings_set",
        serde_json::json!({"default_agent": "codex"}),
    )));
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|e| e.kind.as_str()),
        Some("event.invalid_payload")
    );
    assert_eq!(runtime.snapshot().ui_state.agent_start.kind, None);
}

/// A task's first prompt waits for its agent and is handed out with the
/// start; a definite failure keeps it for Retry, a start settles it.
#[test]
fn a_task_prompt_is_handed_to_its_agent_start_and_kept_for_a_retry() {
    let mut runtime = runtime();
    let id = runtime
        .begin_task_operation(
            "agent_start",
            Some("/tmp/hide-prompt".into()),
            None,
            None,
            Some("claude".into()),
        )
        .unwrap();
    runtime.set_task_agent_launch(
        id,
        Some("  Issue #192를 해결해줘  ".into()),
        vec!["--model".into(), "opus".into()],
    );
    assert!(runtime.ingest_task_operation_result(
        id,
        Ok(live::WorktreeTaskOutcome {
            path: "/tmp/hide-prompt".into(),
            pane_id: "w1:p9".into(),
            purpose_error: None,
            unconfirmed_purpose_token: None,
            issue_error: None,
        }),
    ));
    let pending = runtime.pending_task_agent_start(id).unwrap();
    assert_eq!(pending.prompt.as_deref(), Some("Issue #192를 해결해줘"));
    assert_eq!(pending.args, ["--model", "opus"]);
    assert!(
        runtime.ingest_task_agent_result(id, live::TaskAgentOutcome::Failed("no claude".into()))
    );
    assert!(runtime.task_agent_launch.is_some(), "kept for Retry");
    runtime
        .snapshot
        .task_operation
        .as_mut()
        .unwrap()
        .agent_phase = Some("starting".into());
    assert!(runtime.ingest_task_agent_result(id, live::TaskAgentOutcome::Started));
    assert!(runtime.task_agent_launch.is_none());
    let operation = runtime.snapshot().task_operation.clone().unwrap();
    assert_eq!(operation.agent_phase.as_deref(), Some("started"));
    assert_eq!(operation.agent_message, None);
}

/// A runtime whose project reads Local issues and holds one, `L-1`.
fn local_issue_runtime() -> Runtime {
    let mut runtime = issue_runtime();
    assert!(runtime.dispatch_json(&event(
        "issue_source_set",
        serde_json::json!({"project_path": "/repo", "source": "local"}),
    )));
    assert!(runtime.dispatch_json(&event(
        "issue_create",
        serde_json::json!({"workspace_id": "w", "title": "로컬 이슈", "body": "처음 본문"}),
    )));
    runtime
}

/// A Local issue's panel reads its body and when it was made from the store,
/// and has no labels, author or comments to show.
#[test]
fn a_local_issue_panel_reads_the_body_and_creation_time_with_no_github_fields() {
    let mut runtime = local_issue_runtime();
    assert!(runtime.dispatch_json(&event(
        "issue_detail_request",
        serde_json::json!({"workspace_id": "w", "task_key": "local:/repo#1"}),
    )));
    let detail = runtime.snapshot().issue_work.detail.clone().unwrap();
    assert_eq!(detail.phase, "ready");
    assert_eq!(detail.body.as_deref(), Some("처음 본문"));
    assert!(detail.created_at_unix_ms.is_some());
    assert!(detail.labels.is_empty() && detail.author.is_none() && detail.assignees.is_empty());
    assert_eq!(detail.comment_count, None);
}

/// A GitHub issue's panel read runs on a worker; its answer fills the slot
/// only while that issue is the one being read, so a late answer for an
/// issue read before is dropped.
#[test]
fn a_github_issue_panel_read_settles_only_the_issue_it_asked_for() {
    let mut runtime = issue_runtime();
    runtime.apply_pull_requests();
    assert!(runtime.dispatch_json(&event(
        "issue_detail_request",
        serde_json::json!({"workspace_id": "w", "task_key": "github:acme/project#2"}),
    )));
    let detail = runtime.snapshot().issue_work.detail.clone().unwrap();
    assert_eq!(detail.phase, "failed", "a core without a worker says so");
    assert!(detail.message.unwrap().contains("No worker"));

    runtime.snapshot.issue_work.detail = Some(crate::model::IssueDetailSnapshot::reading(
        "github:acme/project#2".into(),
    ));
    let answer = crate::tasks::TaskDetail {
        body: "본문".into(),
        author: Some("yansfil".into()),
        comment_count: Some(0),
        ..Default::default()
    };
    assert!(!runtime.ingest_issue_detail("github:acme/project#1", Ok(answer.clone())));
    assert!(runtime.ingest_issue_detail("github:acme/project#2", Ok(answer)));
    let detail = runtime.snapshot().issue_work.detail.clone().unwrap();
    assert_eq!(
        (
            detail.phase.as_str(),
            detail.body.as_deref(),
            detail.author.as_deref()
        ),
        ("ready", Some("본문"), Some("yansfil"))
    );
    assert!(!runtime.ingest_issue_detail("github:acme/project#2", Err("late".into())));
}

/// A Local issue's title and body are edited in its panel: the task shows the
/// new title, the next read the new body, and the answer names the request.
#[test]
fn a_local_issue_edit_changes_its_title_and_body_and_answers_the_request() {
    let mut runtime = local_issue_runtime();
    assert!(runtime.dispatch_json(&event(
        "local_issue_update",
        serde_json::json!({"request_id": "r1", "task_key": "local:/repo#1", "title": "  고친 제목 ", "body": "고친 본문"}),
    )));
    let update = runtime.snapshot().issue_work.update.clone().unwrap();
    assert_eq!(
        (
            update.request_id.as_str(),
            update.phase.as_str(),
            update.message
        ),
        ("r1", "ready", None)
    );
    let task = &runtime.snapshot().navigator.workspaces[0].tasks.tasks[0];
    assert_eq!(task.title, "고친 제목");
    assert!(runtime.dispatch_json(&event(
        "issue_detail_request",
        serde_json::json!({"workspace_id": "w", "task_key": "local:/repo#1"}),
    )));
    assert_eq!(
        runtime
            .snapshot()
            .issue_work
            .detail
            .as_ref()
            .unwrap()
            .body
            .as_deref(),
        Some("고친 본문")
    );
}

/// An edit the store refuses leaves the issue as it was and answers with the
/// reason; a GitHub issue has no edit in Hide.
#[test]
fn a_refused_issue_edit_keeps_the_issue_and_says_why() {
    let mut runtime = local_issue_runtime();
    assert!(runtime.dispatch_json(&event(
        "local_issue_update",
        serde_json::json!({"request_id": "r2", "task_key": "local:/repo#1", "title": "  ", "body": "x"}),
    )));
    let update = runtime.snapshot().issue_work.update.clone().unwrap();
    assert_eq!(
        (update.request_id.as_str(), update.phase.as_str()),
        ("r2", "failed")
    );
    assert!(update.message.unwrap().contains("제목"));
    assert_eq!(
        runtime.snapshot().navigator.workspaces[0].tasks.tasks[0].title,
        "로컬 이슈"
    );

    assert!(runtime.dispatch_json(&event(
        "local_issue_update",
        serde_json::json!({"request_id": "r3", "task_key": "local:/repo#9", "title": "없음"}),
    )));
    assert_eq!(
        runtime.snapshot().issue_work.update.as_ref().unwrap().phase,
        "failed"
    );

    assert!(runtime.dispatch_json(&event(
        "local_issue_update",
        serde_json::json!({"request_id": "r4", "task_key": "github:acme/project#2", "title": "GitHub"}),
    )));
    let update = runtime.snapshot().issue_work.update.clone().unwrap();
    assert_eq!(
        (update.request_id.as_str(), update.phase.as_str()),
        ("r4", "failed")
    );
}
