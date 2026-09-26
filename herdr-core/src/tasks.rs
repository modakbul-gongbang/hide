//! A project's tasks in one shape whatever their source is (PRD
//! task-agents-views D-14). The web reads only this shape; each source has an
//! adapter here that projects its own answer into it, and GitHub issues are
//! the one adapter today. The source keeps the authority: Hide reads and
//! opens a task, it never edits one.
//!
//! Every value is additive on the wire and none is a string enum the Swift
//! shell decodes, so the frozen shell reads the snapshot exactly as before.

use serde::Serialize;

use crate::issues::{IssueReference, IssueSnapshot, ProjectIssuesSnapshot};
use crate::model::GithubStatusSnapshot;

/// The source kind of a GitHub issue.
pub const GITHUB: &str = "github";

/// One task. `key` names it across every source and project, so a checkout,
/// a card and (later) a dependency edge can refer to it without knowing the
/// source's own identity scheme.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskSnapshot {
    pub key: String,
    /// The source kind (`github`). A plain string, so a shell that does not
    /// know a later source draws it as an unknown one instead of failing.
    pub source: String,
    /// The id the source shows for it (`#170`, or `acme/other#12` for an
    /// issue of another repository), or none when the source has no ids.
    pub id: Option<String>,
    pub url: Option<String>,
    pub title: String,
    pub open: bool,
    /// The open tasks this one waits on, which may belong to another project
    /// (PRD task-agents-views D-09, D-10); the source records them.
    pub blocked_by: Vec<TaskRefSnapshot>,
}

/// Another task, named by its key and by the id this task's source shows for it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskRefSnapshot {
    pub key: String,
    pub id: Option<String>,
}

/// Where a project's tasks come from and how the last read went.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskSourceSnapshot {
    pub kind: String,
    /// What the operator calls the source (`GitHub`).
    pub label: String,
    /// Which list of that source this is (`acme/app`), once it has answered.
    pub name: Option<String>,
    /// No read has answered yet.
    pub reading: bool,
    /// The last read failed and `tasks` are the answer before it. The detail
    /// is in the diagnostic log; this is the one sentence a tooltip shows.
    pub failure: Option<String>,
    pub last_read_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ProjectTasksSnapshot {
    /// Absent when the project has no source connected.
    pub source: Option<TaskSourceSnapshot>,
    /// Why no source is connected, in the source's own words (`gh` is not
    /// logged in, the repository has no GitHub remote), for the empty state.
    pub unconnected_reason: Option<String>,
    pub tasks: Vec<TaskSnapshot>,
    /// The source listed more than Hide keeps (`ISSUE_LIMIT`).
    pub overflow: bool,
}

/// The key of a GitHub issue.
pub fn github_key(reference: &IssueReference) -> String {
    format!("{GITHUB}:{}", reference.token())
}

/// A `gh` failure that means the project has no GitHub source to read, as
/// opposed to one that could not be read this time.
fn unconnected(status: &GithubStatusSnapshot) -> bool {
    !status.available
        || matches!(
            status.failure_category.as_deref(),
            Some("not installed" | "not logged in" | "no GitHub remote")
        )
}

/// The GitHub adapter: a local Git project's issues and the reader's status.
/// A project that has answered once stays connected, so a later failure keeps
/// its tasks and says so; one that never could be read has no source.
pub fn github_tasks(
    local_git: bool,
    issues: &ProjectIssuesSnapshot,
    status: &GithubStatusSnapshot,
) -> ProjectTasksSnapshot {
    if !local_git {
        return ProjectTasksSnapshot::default();
    }
    let answered = issues.repository.is_some();
    if !answered && !status.loading && unconnected(status) {
        return ProjectTasksSnapshot {
            unconnected_reason: status.unavailable_reason.clone(),
            ..ProjectTasksSnapshot::default()
        };
    }
    let repository = issues.repository.as_deref();
    let source = TaskSourceSnapshot {
        kind: GITHUB.into(),
        label: "GitHub".into(),
        name: issues.repository.clone(),
        reading: !answered && status.unavailable_reason.is_none(),
        failure: status
            .stale
            .then(|| status.unavailable_reason.clone())
            .flatten()
            .or_else(|| {
                issues
                    .dependencies_failure
                    .as_ref()
                    .map(|reason| format!("issue dependencies: {reason}"))
            }),
        last_read_at_unix_ms: status.last_success_at_unix_ms,
    };
    ProjectTasksSnapshot {
        source: Some(source),
        unconnected_reason: None,
        tasks: issues
            .issues
            .iter()
            .map(|issue| github_task(issue, repository))
            .collect(),
        overflow: issues.overflow,
    }
}

/// `#170` in its own repository, `owner/repo#170` from another.
fn github_id(reference: &IssueReference, repository: Option<&str>) -> String {
    if Some(reference.repository.as_str()) == repository {
        format!("#{}", reference.number)
    } else {
        reference.token()
    }
}

fn github_task(issue: &IssueSnapshot, repository: Option<&str>) -> TaskSnapshot {
    let reference = &issue.reference;
    TaskSnapshot {
        key: github_key(reference),
        source: GITHUB.into(),
        id: Some(github_id(reference, repository)),
        url: Some(issue.url.clone()),
        title: issue.title.clone(),
        open: issue.state == "OPEN",
        blocked_by: issue
            .blocked_by
            .iter()
            .map(|blocker| TaskRefSnapshot {
                key: github_key(blocker),
                id: Some(github_id(blocker, repository)),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(repository: &str, number: u32, state: &str) -> IssueSnapshot {
        IssueSnapshot {
            reference: IssueReference {
                repository: repository.into(),
                number,
            },
            title: format!("Issue {number}"),
            url: format!("https://github.com/{repository}/issues/{number}"),
            state: state.into(),
            project_status: None,
            updated_at_unix_ms: None,
            blocked_by: Vec::new(),
        }
    }

    fn answered() -> ProjectIssuesSnapshot {
        ProjectIssuesSnapshot {
            repository: Some("acme/app".into()),
            issues: vec![
                issue("acme/app", 170, "OPEN"),
                issue("acme/other", 12, "CLOSED"),
            ],
            overflow: true,
            dependencies_failure: None,
        }
    }

    fn healthy() -> GithubStatusSnapshot {
        GithubStatusSnapshot {
            available: true,
            last_success_at_unix_ms: Some(5),
            ..Default::default()
        }
    }

    #[test]
    fn issues_become_source_neutral_tasks() {
        let tasks = github_tasks(true, &answered(), &healthy());
        let source = tasks.source.expect("connected");
        assert_eq!(source.name.as_deref(), Some("acme/app"));
        assert!(!source.reading && source.failure.is_none());
        assert!(tasks.overflow);
        assert_eq!(tasks.tasks[0].key, "github:acme/app#170");
        assert_eq!(tasks.tasks[0].id.as_deref(), Some("#170"));
        assert!(tasks.tasks[0].open);
        // Another repository's issue keeps its repository in the id.
        assert_eq!(tasks.tasks[1].id.as_deref(), Some("acme/other#12"));
        assert!(!tasks.tasks[1].open);
    }

    #[test]
    fn a_blocker_is_named_by_key_and_by_the_id_this_repository_shows() {
        let mut issues = answered();
        issues.issues[0].blocked_by = vec![
            IssueReference {
                repository: "acme/app".into(),
                number: 169,
            },
            IssueReference {
                repository: "acme/other".into(),
                number: 12,
            },
        ];
        let tasks = github_tasks(true, &issues, &healthy());
        assert_eq!(
            tasks.tasks[0].blocked_by,
            vec![
                TaskRefSnapshot {
                    key: "github:acme/app#169".into(),
                    id: Some("#169".into())
                },
                TaskRefSnapshot {
                    key: "github:acme/other#12".into(),
                    id: Some("acme/other#12".into())
                },
            ]
        );
    }

    #[test]
    fn unread_dependencies_are_a_source_failure_beside_current_tasks() {
        let mut issues = answered();
        issues.dependencies_failure = Some("Field 'blockedBy' doesn't exist".into());
        let tasks = github_tasks(true, &issues, &healthy());
        assert_eq!(tasks.tasks.len(), 2);
        assert_eq!(
            tasks.source.and_then(|source| source.failure).as_deref(),
            Some("issue dependencies: Field 'blockedBy' doesn't exist")
        );
    }

    #[test]
    fn a_failed_read_keeps_the_tasks_and_says_so() {
        let status = GithubStatusSnapshot {
            available: true,
            stale: true,
            unavailable_reason: Some("rate limited".into()),
            failure_category: Some("network or rate limit".into()),
            last_success_at_unix_ms: Some(5),
            ..Default::default()
        };
        let tasks = github_tasks(true, &answered(), &status);
        assert_eq!(tasks.tasks.len(), 2);
        assert_eq!(
            tasks.source.and_then(|source| source.failure).as_deref(),
            Some("rate limited")
        );
    }

    #[test]
    fn a_project_gh_cannot_read_has_no_source_and_says_why() {
        for category in ["not installed", "not logged in", "no GitHub remote"] {
            let status = GithubStatusSnapshot {
                available: category == "no GitHub remote",
                stale: true,
                unavailable_reason: Some(format!("gh: {category}")),
                failure_category: Some(category.into()),
                ..Default::default()
            };
            let tasks = github_tasks(true, &ProjectIssuesSnapshot::default(), &status);
            assert!(tasks.source.is_none(), "{category}");
            assert_eq!(
                tasks.unconnected_reason,
                Some(format!("gh: {category}")),
                "{category}"
            );
        }
    }

    #[test]
    fn a_first_read_in_flight_is_a_source_still_reading() {
        let status = GithubStatusSnapshot {
            loading: true,
            ..Default::default()
        };
        let tasks = github_tasks(true, &ProjectIssuesSnapshot::default(), &status);
        assert!(tasks.source.expect("reading").reading);
    }

    #[test]
    fn a_folder_or_device_project_has_no_source() {
        assert_eq!(
            github_tasks(false, &answered(), &healthy()),
            ProjectTasksSnapshot::default()
        );
    }
}
