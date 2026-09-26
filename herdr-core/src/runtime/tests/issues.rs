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
