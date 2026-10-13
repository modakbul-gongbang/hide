use super::*;
use crate::project_settings::{
    BaseBranchChoice, IssueSourceChoice, RepositoryConnection, WriteState,
};

fn fixture() -> Runtime {
    let mut runtime = runtime();
    let mut project = workspace(
        "visible-id",
        "Project",
        "/fixture/project",
        vec![checkout(
            "visible-id",
            "feature-checkout",
            "/fixture/feature",
            None,
        )],
    );
    project.is_git = true;
    project.branches = vec!["main".into(), "develop".into()];
    runtime.snapshot.navigator.workspaces = vec![project];
    runtime
}

fn change(runtime: &mut Runtime, request: &str, patch: serde_json::Value) -> bool {
    runtime.dispatch_json(&serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION, "kind": "project_settings_set", "payload": {
            "device_id": crate::node::TEST_NODE, "root": "/fixture/project", "request_id": request, "patch": patch
        }
    })).unwrap())
}

#[test]
fn project_settings_save_survives_restart_and_omitted_fields() {
    let mut runtime = fixture();
    let id = crate::project_settings::key(crate::node::TEST_NODE, "/fixture/project");
    assert!(change(
        &mut runtime,
        "repository",
        serde_json::json!({"repository": {"mode": "repository", "repository": "example/override"}})
    ));
    assert_eq!(
        runtime.snapshot.ui_state.project_settings_writes[&id].state,
        WriteState::Saved
    );
    assert!(change(
        &mut runtime,
        "source",
        serde_json::json!({"issue_source": "local"})
    ));
    let (loaded, _, _) = crate::persistence::load(&runtime.state_path);
    let record = &loaded.project_settings[&id];
    assert_eq!(
        record.repository,
        Some(RepositoryConnection::Repository {
            repository: "example/override".into()
        })
    );
    assert_eq!(record.issue_source, Some(IssueSourceChoice::Local));
    assert!(
        loaded.project_settings_writes.is_empty(),
        "receipts are runtime-only"
    );
    assert!(
        !change(
            &mut runtime,
            "source",
            serde_json::json!({"issue_source": "github"})
        ),
        "same intent cannot be replayed as a different write"
    );
    assert_eq!(
        runtime.snapshot.ui_state.project_settings[&id].issue_source,
        Some(IssueSourceChoice::Local)
    );
}

#[test]
fn invalid_repository_and_failed_save_keep_last_confirmed_value() {
    let mut runtime = fixture();
    let id = crate::project_settings::key(crate::node::TEST_NODE, "/fixture/project");
    change(
        &mut runtime,
        "confirmed",
        serde_json::json!({"issue_source": "github"}),
    );
    change(
        &mut runtime,
        "invalid",
        serde_json::json!({"repository": {"mode": "repository", "repository": "https://github.com/example/repo"}}),
    );
    assert_eq!(
        runtime.snapshot.ui_state.project_settings_writes[&id]
            .error
            .as_deref(),
        Some("project_settings_invalid_repository")
    );
    let blocked = runtime.state_path.with_file_name("blocked");
    std::fs::write(&blocked, b"not a directory").unwrap();
    runtime.state_path = blocked.join("state.json");
    change(
        &mut runtime,
        "failed",
        serde_json::json!({"issue_source": "local"}),
    );
    assert_eq!(
        runtime.snapshot.ui_state.project_settings[&id].issue_source,
        Some(IssueSourceChoice::Github)
    );
    assert_eq!(
        runtime.snapshot.ui_state.project_settings_writes[&id]
            .error
            .as_deref(),
        Some("project_settings_save_failed")
    );
    assert!(runtime.project_settings_pending.is_empty());
    assert_eq!(
        runtime.snapshot.ui_state.project_settings_writes[&id]
            .patch
            .issue_source,
        Some(IssueSourceChoice::Local),
        "the refused intent remains available for Retry after the dialog closes"
    );
}

#[test]
fn automatic_choice_shadows_legacy_and_uses_main_root_not_primary_checkout() {
    let mut runtime = fixture();
    runtime
        .snapshot
        .ui_state
        .project_issue_sources
        .insert("/fixture/project".into(), "local".into());
    runtime
        .snapshot
        .ui_state
        .project_base_branches
        .insert("/fixture/project".into(), "develop".into());
    let id = crate::project_settings::key(crate::node::TEST_NODE, "/fixture/project");
    let resolve = |runtime: &Runtime| {
        crate::project_settings::issue_choice(
            &runtime.snapshot.ui_state.project_settings,
            &runtime.snapshot.ui_state.project_issue_sources,
            crate::node::TEST_NODE,
            "/fixture/project",
        )
        .map(str::to_owned)
    };
    assert_eq!(resolve(&runtime).as_deref(), Some("local"));
    change(
        &mut runtime,
        "auto",
        serde_json::json!({"issue_source": "auto", "base_branch": {"mode": "auto"}}),
    );
    assert_eq!(resolve(&runtime).as_deref(), Some("auto"));
    assert_eq!(
        runtime.snapshot.ui_state.project_settings[&id].base_branch,
        Some(BaseBranchChoice::Auto)
    );
    assert_eq!(
        crate::project_settings::base_choice(
            &runtime.snapshot.ui_state.project_settings,
            &runtime.snapshot.ui_state.project_base_branches,
            crate::node::TEST_NODE,
            "/fixture/project"
        ),
        None
    );
    assert_ne!(
        id,
        crate::project_settings::key("other-device", "/fixture/project")
    );
    assert_ne!(
        id,
        crate::project_settings::key(crate::node::TEST_NODE, "/fixture/feature")
    );
}

#[test]
fn disconnected_same_path_does_not_write_local_project() {
    let mut runtime = fixture();
    let before = runtime.snapshot.ui_state.project_settings.clone();
    runtime.dispatch_json(&serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION, "kind": "project_settings_set", "payload": {
            "device_id": "disconnected-device", "root": "/fixture/project", "request_id": "remote", "patch": {"issue_source": "local"}
        }
    })).unwrap());
    assert_eq!(runtime.snapshot.ui_state.project_settings, before);
    let id = crate::project_settings::key("disconnected-device", "/fixture/project");
    assert_eq!(
        runtime.snapshot.ui_state.project_settings_writes[&id].state,
        WriteState::Failed
    );
}

#[test]
fn unacknowledged_save_keeps_consumers_on_confirmed_repository() {
    let mut runtime = fixture();
    // A disconnected worker owner leaves the save pending. Drive exact save
    // acknowledgments separately, without timing a filesystem worker.
    runtime.worker_context = Some(RuntimeWorkerContext {
        runtime: std::sync::Weak::new(),
        notifier: crate::handle::ChangeNotifier::noop(),
    });
    let id = crate::project_settings::key(crate::node::TEST_NODE, "/fixture/project");
    change(
        &mut runtime,
        "pending",
        serde_json::json!({"repository": {"mode": "repository", "repository": "example/second"}}),
    );
    assert_eq!(
        runtime.snapshot.ui_state.project_settings_writes[&id].state,
        WriteState::Pending
    );
    assert_eq!(runtime.github_request().projects[0].repository, None);
    let saved = runtime.ui_state_to_save();
    assert!(
        !runtime.acknowledge_project_settings_save(&UiStateSnapshot::default(), true),
        "unrelated save cannot acknowledge this intent"
    );
    assert_eq!(
        runtime.snapshot.ui_state.project_settings_writes[&id].state,
        WriteState::Pending
    );
    assert!(runtime.acknowledge_project_settings_save(&saved, true));
    assert_eq!(
        runtime.github_request().projects[0].repository.as_deref(),
        Some("example/second")
    );
    assert_eq!(
        runtime.snapshot.ui_state.project_settings_writes[&id].state,
        WriteState::Saved
    );
}

#[test]
fn repository_change_clears_old_success_and_rejects_old_generation() {
    let mut runtime = fixture();
    runtime.ingest_github(crate::model::GithubSnapshot {
        projects: vec![crate::model::GithubProjectSnapshot {
            root_path: "/fixture/project".into(),
            issues: crate::issues::ProjectIssuesSnapshot {
                repository: Some("example/first".into()),
                ..Default::default()
            },
            status: crate::model::GithubStatusSnapshot {
                available: true,
                ..Default::default()
            },
            issues_read: true,
            pull_requests_read: true,
            ..Default::default()
        }],
    });
    let old_request = runtime.github_request();
    let old_answer = runtime.github.clone();
    change(
        &mut runtime,
        "switch",
        serde_json::json!({"repository": {"mode": "repository", "repository": "example/second"}}),
    );
    assert!(runtime.github.project("/fixture/project").is_none());
    assert_ne!(old_request, runtime.github_request());
    assert!(!runtime.ingest_github_answer(old_answer, old_request == runtime.github_request()));
    assert!(
        runtime.snapshot.navigator.workspaces[0]
            .home_issues
            .issues
            .is_empty()
    );
}

#[test]
fn repository_change_keeps_bare_legacy_links_scoped_after_restart() {
    let mut runtime = fixture();
    let checkout_id = runtime.snapshot.navigator.workspaces[0].checkouts[0]
        .id
        .clone();
    runtime.snapshot.navigator.workspaces[0].checkouts[0].branch = Some("7-task".into());
    runtime.issue_candidates.insert(
        checkout_id.clone(),
        crate::issues::IssueCandidate {
            reference: crate::issues::IssueReference {
                repository: "example/first".into(),
                number: 7,
            },
            source: "브랜치 이름".into(),
        },
    );
    change(
        &mut runtime,
        "switch",
        serde_json::json!({"repository": {"mode": "repository", "repository": "example/second"}}),
    );
    let (saved, _, _) = crate::persistence::load(&runtime.state_path);
    runtime.snapshot.ui_state = saved;
    runtime.issue_candidates.clear();
    runtime.snapshot.navigator.workspaces[0]
        .home_issues
        .repository = Some("example/second".into());
    runtime.sync_issues();
    assert_eq!(
        runtime.issue_candidates[&checkout_id].reference.repository,
        "example/first"
    );
    // A new full reference is an explicit choice, even for the same number.
    runtime.snapshot.navigator.workspaces[0].checkouts[0].branch_issue =
        Some("example/second#7".into());
    runtime.sync_issues();
    assert_eq!(
        runtime.issue_candidates[&checkout_id].reference.repository,
        "example/second"
    );
}

#[test]
fn old_pr_body_answer_cannot_link_same_number_in_new_repository() {
    let mut runtime = fixture();
    let root = "/fixture/project";
    let reference = crate::issues::IssueReference {
        repository: "example/first".into(),
        number: 7,
    };
    runtime
        .github
        .projects
        .push(crate::model::GithubProjectSnapshot {
            root_path: root.into(),
            issues: crate::issues::ProjectIssuesSnapshot {
                repository: Some("example/second".into()),
                ..Default::default()
            },
            pull_requests: vec![crate::model::PullRequestSnapshot {
                number: 3,
                url: "https://github.com/example/second/pull/3".into(),
                head_branch: "7-task".into(),
                base_branch: "main".into(),
                title: "second PR".into(),
                badge: crate::model::PullRequestBadge::Open,
                checks: crate::model::PullRequestChecks::Passing,
                review: None,
                is_draft: false,
                closing_issues: vec![],
                merged_at_unix_ms: None,
                updated_at_unix_ms: None,
                created_at_unix_ms: None,
                closed_at_unix_ms: None,
                head_oid: None,
                cross_repository: false,
            }],
            ..Default::default()
        });
    runtime.snapshot.pr_work.link = Some(crate::model::PrLinkSnapshot {
        request_id: "old-intent".into(),
        workspace_id: "visible-id".into(),
        pr_number: 3,
        step: "body".into(),
        phase: "working".into(),
        issue_key: Some(crate::tasks::github_key(&reference)),
        issue_id: Some("#7".into()),
        created: false,
        message: None,
    });
    assert!(runtime.finish_pr_body("old-intent", &reference, Ok(true)));
    assert!(
        runtime.github.project(root).unwrap().pull_requests[0]
            .closing_issues
            .is_empty()
    );
    assert_eq!(
        runtime
            .snapshot
            .pr_work
            .link
            .as_ref()
            .unwrap()
            .issue_key
            .as_deref(),
        Some("github:example/first#7")
    );
}
