//! A project's tasks in one shape whatever their source is (PRD
//! task-agents-views D-14). The web reads only this shape; each source has an
//! adapter here that projects its own answer into it: GitHub issues read
//! through the operator's `gh`, and Local issues kept on this Mac
//! (`local_issues.rs`). Every local project has exactly one source, the one
//! the operator chose in Settings › Issues or, by default, GitHub when the
//! repository reads as a GitHub repository and Local otherwise, so no project
//! is ever "not connected".
//!
//! Every value is additive on the wire and none is a string enum an older
//! reader decodes, so such a reader takes the snapshot exactly as before.

use serde::Serialize;

use crate::issues::{IssueReference, IssueSnapshot, ProjectIssuesSnapshot};
use crate::local_issues::LocalIssueProject;
use crate::model::GithubStatusSnapshot;

/// The source kind of a GitHub issue.
pub const GITHUB: &str = "github";
/// The source kind of an issue kept on this Mac.
pub const LOCAL: &str = "local";

/// One task. `key` names it across every source and project, so a checkout,
/// a card and a dependency edge can refer to it without knowing the source's
/// own identity scheme.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskSnapshot {
    pub key: String,
    /// The source kind (`github`, `local`). A plain string, so a shell that
    /// does not know a later source draws it as an unknown one instead of
    /// failing.
    pub source: String,
    /// The id the source shows for it (`#170`, `acme/other#12` for an issue of
    /// another repository, `L-3` for a local issue).
    pub id: Option<String>,
    pub url: Option<String>,
    pub title: String,
    pub open: bool,
    /// When the source last changed it, for the backlog's order and age.
    pub updated_at_unix_ms: Option<u64>,
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
    /// What the operator calls the source (`GitHub`, `Local`).
    pub label: String,
    /// Which list of that source this is (`acme/app`), once it has answered.
    pub name: Option<String>,
    /// No read has answered yet.
    pub reading: bool,
    /// The last read failed and `tasks` are the answer before it. The detail
    /// is in the diagnostic log; this is the one sentence a tooltip shows.
    pub failure: Option<String>,
    pub last_read_at_unix_ms: Option<u64>,
    /// The operator chose this source in Settings rather than the default.
    pub chosen: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ProjectTasksSnapshot {
    /// Absent only for a device's project, whose issues this Mac does not read.
    pub source: Option<TaskSourceSnapshot>,
    pub tasks: Vec<TaskSnapshot>,
    /// The source listed more than Hide keeps (`ISSUE_LIMIT`).
    pub overflow: bool,
}

/// The key of a GitHub issue.
pub fn github_key(reference: &IssueReference) -> String {
    format!("{GITHUB}:{}", reference.token())
}

/// The key of a local issue: the project's path names it across projects.
pub fn local_key(project_path: &str, number: u32) -> String {
    format!("{LOCAL}:{project_path}#{number}")
}

/// The project and number a local key names.
pub fn parse_local_key(key: &str) -> Option<(&str, u32)> {
    let rest = key.strip_prefix("local:")?;
    let (path, number) = rest.rsplit_once('#')?;
    Some((
        path,
        number.parse().ok().filter(|number: &u32| *number > 0)?,
    ))
}

/// The GitHub reference a GitHub key names.
pub fn parse_github_key(key: &str) -> Option<IssueReference> {
    IssueReference::parse(key.strip_prefix("github:")?, None).ok()
}

/// The token a checkout's issue link stores for a task key: `owner/repo#N`
/// for a GitHub issue, `L-N` for a local one. The linking chain reads both
/// back (`runtime/issues.rs`).
pub fn issue_token(key: &str) -> Option<String> {
    if let Some((_, number)) = parse_local_key(key) {
        return Some(crate::local_issues::display_id(number));
    }
    parse_github_key(key).map(|reference| reference.token())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceKind {
    Github,
    Local,
}

/// The source a local project reads. `choice` is the operator's
/// (`ui_state.project_issue_sources`); without one, a Git repository reads
/// GitHub unless `gh` said it has no GitHub remote or is not installed, and a
/// folder reads Local. A GitHub read that fails for another reason (logged
/// out, offline) stays GitHub and says so, rather than moving the project's
/// issues somewhere else under the operator.
pub fn source_kind(
    choice: Option<&str>,
    git: bool,
    issues: &ProjectIssuesSnapshot,
    status: &GithubStatusSnapshot,
) -> SourceKind {
    match choice {
        Some(LOCAL) => SourceKind::Local,
        Some(GITHUB) if git => SourceKind::Github,
        _ if !git => SourceKind::Local,
        _ if issues.repository.is_some() => SourceKind::Github,
        _ if matches!(
            status.failure_category.as_deref(),
            Some("not installed" | "no GitHub remote")
        ) =>
        {
            SourceKind::Local
        }
        _ => SourceKind::Github,
    }
}

/// How the Local store stands for one project.
pub enum LocalRead<'a> {
    /// The store could not be read; its issues are unknown, not empty.
    Failed(&'a str),
    Ready(Option<&'a LocalIssueProject>),
}

/// The GitHub adapter: a local Git project's issues and the reader's status.
/// A read that has not answered yet is a source still reading; a failed one
/// keeps the tasks read before it and says so.
pub fn github_tasks(
    issues: &ProjectIssuesSnapshot,
    status: &GithubStatusSnapshot,
    chosen: bool,
) -> ProjectTasksSnapshot {
    let answered = issues.repository.is_some();
    let repository = issues.repository.as_deref();
    let failure = if status.stale || (!answered && !status.loading) {
        status.unavailable_reason.clone()
    } else {
        None
    };
    let source = TaskSourceSnapshot {
        kind: GITHUB.into(),
        label: "GitHub".into(),
        name: issues.repository.clone(),
        reading: !answered && failure.is_none(),
        failure: failure.or_else(|| {
            issues
                .dependencies_failure
                .as_ref()
                .map(|reason| format!("issue dependencies: {reason}"))
        }),
        last_read_at_unix_ms: status.last_success_at_unix_ms,
        chosen,
    };
    ProjectTasksSnapshot {
        source: Some(source),
        tasks: issues
            .issues
            .iter()
            .map(|issue| github_task(issue, repository))
            .collect(),
        overflow: issues.overflow,
    }
}

/// The Local adapter: the project's issues in the store on this Mac.
pub fn local_tasks(project_path: &str, read: LocalRead<'_>, chosen: bool) -> ProjectTasksSnapshot {
    let (failure, project) = match read {
        LocalRead::Failed(reason) => (Some(reason.to_owned()), None),
        LocalRead::Ready(project) => (None, project),
    };
    ProjectTasksSnapshot {
        source: Some(TaskSourceSnapshot {
            kind: LOCAL.into(),
            label: "Local".into(),
            name: None,
            reading: false,
            failure,
            last_read_at_unix_ms: None,
            chosen,
        }),
        tasks: project
            .map(|project| {
                // Newest first, the order GitHub lists issues in.
                project
                    .issues
                    .iter()
                    .rev()
                    .map(|issue| TaskSnapshot {
                        key: local_key(project_path, issue.number),
                        source: LOCAL.into(),
                        id: Some(crate::local_issues::display_id(issue.number)),
                        url: None,
                        title: issue.title.clone(),
                        open: issue.open,
                        updated_at_unix_ms: Some(issue.updated_at_unix_ms),
                        blocked_by: Vec::new(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        overflow: false,
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
        updated_at_unix_ms: issue.updated_at_unix_ms,
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
        let tasks = github_tasks(&answered(), &healthy(), false);
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
        let tasks = github_tasks(&issues, &healthy(), false);
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
        let tasks = github_tasks(&issues, &healthy(), false);
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
        let tasks = github_tasks(&answered(), &status, false);
        assert_eq!(tasks.tasks.len(), 2);
        assert_eq!(
            tasks.source.and_then(|source| source.failure).as_deref(),
            Some("rate limited")
        );
    }

    #[test]
    fn without_a_choice_a_repository_gh_cannot_see_on_github_reads_local() {
        let none = ProjectIssuesSnapshot::default();
        for (category, expected) in [
            ("not installed", SourceKind::Local),
            ("no GitHub remote", SourceKind::Local),
            // Logged out or offline is still a GitHub repository: it stays
            // GitHub and says why it could not be read.
            ("not logged in", SourceKind::Github),
            ("network or rate limit", SourceKind::Github),
        ] {
            let status = GithubStatusSnapshot {
                unavailable_reason: Some(format!("gh: {category}")),
                failure_category: Some(category.into()),
                ..Default::default()
            };
            assert_eq!(
                source_kind(None, true, &none, &status),
                expected,
                "{category}"
            );
        }
        // A folder has no GitHub to read; a choice wins over the default.
        assert_eq!(
            source_kind(None, false, &none, &healthy()),
            SourceKind::Local
        );
        assert_eq!(
            source_kind(Some(LOCAL), true, &answered(), &healthy()),
            SourceKind::Local
        );
        assert_eq!(
            source_kind(Some(GITHUB), false, &none, &healthy()),
            SourceKind::Local
        );
    }

    #[test]
    fn a_github_read_that_never_answered_says_why_instead_of_reading_forever() {
        let status = GithubStatusSnapshot {
            unavailable_reason: Some("gh auth login".into()),
            failure_category: Some("not logged in".into()),
            ..Default::default()
        };
        let tasks = github_tasks(&ProjectIssuesSnapshot::default(), &status, false);
        let source = tasks.source.expect("a GitHub source");
        assert!(!source.reading);
        assert_eq!(source.failure.as_deref(), Some("gh auth login"));
    }

    #[test]
    fn local_issues_are_tasks_keyed_by_project_newest_first() {
        let mut store = crate::local_issues::LocalIssueStore::default();
        store.create("/p", "첫 이슈", "", 1).unwrap();
        store.create("/p", "둘째", "", 2).unwrap();
        store.set_open("/p", 1, false, 3);
        let tasks = local_tasks("/p", LocalRead::Ready(store.project("/p")), true);
        let source = tasks.source.expect("local");
        assert_eq!(
            (source.kind.as_str(), source.chosen, source.reading),
            (LOCAL, true, false)
        );
        assert_eq!(tasks.tasks[0].id.as_deref(), Some("L-2"));
        assert_eq!(tasks.tasks[0].key, "local:/p#2");
        assert!(!tasks.tasks[1].open);
        assert_eq!(parse_local_key("local:/p#2"), Some(("/p", 2)));
        assert_eq!(
            local_tasks("/p", LocalRead::Failed("damaged"), false)
                .source
                .unwrap()
                .failure
                .as_deref(),
            Some("damaged")
        );
    }

    #[test]
    fn a_first_read_in_flight_is_a_source_still_reading() {
        let status = GithubStatusSnapshot {
            loading: true,
            ..Default::default()
        };
        let tasks = github_tasks(&ProjectIssuesSnapshot::default(), &status, false);
        assert!(tasks.source.expect("reading").reading);
    }
}
