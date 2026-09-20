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
