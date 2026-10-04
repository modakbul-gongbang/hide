//! Each open repository's pull requests, read through the operator's own `gh`.
//!
//! Hide holds no GitHub token and makes no network call: every remote fact
//! here comes from a `gh` subprocess that authenticates as the person running
//! the app. That is the whole security boundary, and it is why "gh is not
//! installed" and "gh is not logged in" are first-class states rather than
//! errors - they are the normal condition of a machine that has not opted in.
//!
//! Like the worktree reader, the subprocess runs on a worker thread: `gh pr
//! list` reaches the network and routinely takes a second or more, which would
//! otherwise be a second of coordinator latency for every Herdr pane event.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use hide_platform::process::OwnedChild;
use serde::Deserialize;

use crate::model::{
    GithubProjectSnapshot, GithubSnapshot, GithubStatusSnapshot, PullRequestBadge,
    PullRequestChecks, PullRequestSnapshot, ReviewDecision,
};
use crate::reader::BackgroundRead;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);

/// What the list of every pull request asks for. `statusCheckRollup` is
/// left out: asking GitHub for the checks of all 200 pull requests, merged
/// and closed ones included, is what made one read take 11 - 14 seconds
/// against the 15-second limit, and nothing draws a settled pull request's
/// checks as news.
const PULL_REQUEST_FIELDS: &str = "number,title,headRefName,headRefOid,isCrossRepository,baseRefName,state,reviewDecision,isDraft,url,mergedAt,updatedAt,createdAt,closedAt,closingIssuesReferences";

/// The second list: only the open pull requests, only their checks.
const OPEN_CHECK_FIELDS: &str = "number,statusCheckRollup";

/// Every pull request `gh` will return in one call. Past this, older pull
/// requests are simply absent and their branches read as having none; the
/// limit is stated here so the risk is findable from the code that takes it.
pub(crate) const PULL_REQUEST_LIMIT: &str = "200";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GithubProjectRequest {
    pub links: Vec<crate::issues::IssueReference>,
    /// A path inside the repository.
    pub root: PathBuf,
    /// Bumped on Git section opening and explicit header refresh.
    /// Each repository's generation coalesces repeated requests independently.
    pub generation: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GithubRequest {
    pub projects: Vec<GithubProjectRequest>,
}

/// Why one project is read on this pass, stated in the read log so "did the
/// refresh button / section opening actually
/// re-read?" is answered afterwards without a debugger (G5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadReason {
    First,
    Generation,
}

impl ReadReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Generation => "generation",
        }
    }
}

/// Whether a project must be read again, given what the reader last answered
/// for it: nothing yet or an answer for an older request. An answer for the
/// same request is reused indefinitely, which
/// is what keeps one project's trigger from re-reading every other project.
fn read_reason(cached: Option<u64>, generation: u64) -> Option<ReadReason> {
    match cached {
        None => Some(ReadReason::First),
        Some(read_generation) if read_generation != generation => Some(ReadReason::Generation),
        Some(_) => None,
    }
}

/// A pull request's check rollup as it was last read, by number and head
/// commit. A settled pull request's checks are not asked for again, so this
/// is where one that was read while it was open is still found.
type KnownChecks = HashMap<(u32, String), PullRequestChecks>;

struct CachedProject {
    generation: u64,
    answer: GithubProjectSnapshot,
    /// Kept across a failed read, so a pull request that merged while GitHub
    /// was unreachable still has the checks it was last seen with.
    checks: KnownChecks,
}

/// The last answer per requested root, kept on the worker's side so a pass
/// re-reads only the projects that are due and hands the rest back as they
/// were. It is the reader's own state, never the runtime's: the runtime
/// receives a whole snapshot every time and does not know which half is new.
type Cache = Arc<Mutex<HashMap<PathBuf, CachedProject>>>;

pub struct GithubReader {
    inner: BackgroundRead<GithubRequest, GithubAnswer>,
}

pub struct GithubAnswer {
    pub request: GithubRequest,
    pub snapshot: GithubSnapshot,
}

impl GithubReader {
    pub fn new() -> Self {
        let cache: Cache = Arc::default();
        Self {
            inner: BackgroundRead::on_change(Duration::ZERO, move |request: &GithubRequest| {
                GithubAnswer {
                    request: request.clone(),
                    snapshot: read(&cache, request),
                }
            }),
        }
    }

    pub fn read_if_due(&mut self, request: GithubRequest) -> Option<GithubAnswer> {
        self.inner.poll(request)
    }
}

impl Default for GithubReader {
    fn default() -> Self {
        Self::new()
    }
}

fn read(cache: &Mutex<HashMap<PathBuf, CachedProject>>, request: &GithubRequest) -> GithubSnapshot {
    // The worker is the only thread that touches the cache, and one worker
    // runs at a time; the lock exists so the closure can be shared with it.
    let mut cache = cache.lock().unwrap_or_else(PoisonError::into_inner);
    cache.retain(|root, _| request.projects.iter().any(|project| &project.root == root));
    let due: Vec<(&GithubProjectRequest, ReadReason)> = request
        .projects
        .iter()
        .filter_map(|project| {
            let cached = cache.get(&project.root).map(|cached| cached.generation);
            read_reason(cached, project.generation).map(|reason| (project, reason))
        })
        .collect();
    if !due.is_empty() {
        // One line per pass naming each project read and why, with the
        // generation that asked for it.
        crate::diagnostic!(serde_json::json!({
            "component": "github",
            "kind": "pull_requests.read",
            "projects": due
                .iter()
                .map(|(project, reason)| serde_json::json!({
                    "project": project.root.to_string_lossy(),
                    "generation": project.generation,
                    "reason": reason.as_str(),
                }))
                .collect::<Vec<_>>(),
        }));
        // One authentication check for the pass, not one per repository: `gh
        // auth status` is the same answer every time and it is the expensive
        // half of an unauthenticated machine's cost.
        let authentication = authentication();
        for (project, _) in due {
            let known = cache
                .get(&project.root)
                .map(|cached| cached.checks.clone())
                .unwrap_or_default();
            let answer = read_root(
                &authentication,
                &project.root,
                project.generation,
                &project.links,
                &known,
            );
            let checks = carried_checks(&answer, known);
            cache.insert(
                project.root.clone(),
                CachedProject {
                    generation: project.generation,
                    answer,
                    checks,
                },
            );
        }
    }
    let mut projects: Vec<GithubProjectSnapshot> = Vec::new();
    for project in &request.projects {
        let Some(cached) = cache.get(&project.root) else {
            continue;
        };
        // Two navigator entries inside one repository share its answer.
        if cached.answer.root_path.is_empty()
            || projects
                .iter()
                .any(|known| known.root_path == cached.answer.root_path)
        {
            continue;
        }
        projects.push(cached.answer.clone());
    }
    GithubSnapshot { projects }
}

/// One repository's answer, or an empty `root_path` when the path is not
/// inside a git repository at all and so has nothing to report.
fn read_root(
    authentication: &Result<(), GhFailure>,
    root: &Path,
    generation: u64,
    links: &[crate::issues::IssueReference],
    known: &KnownChecks,
) -> GithubProjectSnapshot {
    let started = Instant::now();
    let Some(main) = main_worktree(root) else {
        return GithubProjectSnapshot::default();
    };
    let root_path = hide_platform::path::to_wire_lossy(&main);
    let project = match authentication {
        Err(reason) => GithubProjectSnapshot {
            root_path,
            status: GithubStatusSnapshot {
                available: false,
                unavailable_reason: Some(reason.reason.clone()),
                failure_category: Some(reason.category.to_owned()),
                ..GithubStatusSnapshot::default()
            },
            ..GithubProjectSnapshot::default()
        },
        Ok(()) => read_project(&main, root_path, links, known),
    };
    // A failure and an empty answer are both stated, separately: an empty
    // list with no reason is a repository with no pull requests.
    crate::diagnostic!(serde_json::json!({
        "component": "github",
        "kind": if project.status.unavailable_reason.is_some() {
            "pull_requests.failed"
        } else if project.pull_requests.is_empty() {
            "pull_requests.empty"
        } else {
            "pull_requests.ok"
        },
        "generation": generation,
        "project": project.root_path,
        "available": project.status.available,
        "pull_requests": project.pull_requests.len(),
        "duration_ms": started.elapsed().as_millis() as u64,
        "message": project.status.unavailable_reason,
    }));
    project
}

/// Authentication is read-only and uses the same bounded subprocess as PR listing.
fn authentication() -> Result<(), GhFailure> {
    gh(None, &["auth", "status"]).map(|_| ())
}

fn read_project(
    root: &Path,
    root_path: String,
    links: &[crate::issues::IssueReference],
    known: &KnownChecks,
) -> GithubProjectSnapshot {
    let pull_requests = match list_pull_requests(&|arguments| gh(Some(root), arguments), known) {
        Ok(value) => value,
        Err(reason) => {
            return GithubProjectSnapshot {
                root_path,
                status: failed(reason),
                ..GithubProjectSnapshot::default()
            };
        }
    };
    match read_issues(root, links) {
        Ok((issues, warning)) => GithubProjectSnapshot {
            root_path,
            issues,
            pull_requests,
            pull_requests_read: true,
            issues_read: true,
            status: GithubStatusSnapshot {
                available: true,
                loading: false,
                stale: false,
                last_success_at_unix_ms: Some(now_unix_ms()),
                unavailable_reason: warning
                    .as_ref()
                    .map(|reason| format!("Project metadata unavailable: {}", reason.reason)),
                failure_category: warning.map(|reason| reason.category.to_owned()),
            },
        },
        Err(reason) => GithubProjectSnapshot {
            root_path,
            pull_requests,
            pull_requests_read: true,
            status: failed(reason),
            ..Default::default()
        },
    }
}

/// Every pull request, with the checks of the open ones.
///
/// Two lists, asked for at the same time under their own 15-second limits:
/// the 200 newest pull requests of any state without their checks, and the
/// open ones with nothing but their checks. A settled pull request's checks
/// are not asked for again: the ones read while it was open (`known`, by
/// number and head commit) stay, and without them its checks are `Unknown`,
/// never a guess. Either list failing fails the read, so half an answer is
/// never taken for a repository with no pull requests.
fn list_pull_requests(
    run: &(impl Fn(&[&str]) -> Result<String, GhFailure> + Sync),
    known: &KnownChecks,
) -> Result<Vec<PullRequestSnapshot>, GhFailure> {
    let list = |state: &str, fields: &str| {
        run(&[
            "pr",
            "list",
            "--state",
            state,
            "--limit",
            PULL_REQUEST_LIMIT,
            "--json",
            fields,
        ])
    };
    let (every, open) = std::thread::scope(|scope| {
        let open = scope.spawn(|| list("open", OPEN_CHECK_FIELDS));
        let every = list("all", PULL_REQUEST_FIELDS);
        (every, open.join())
    });
    let every = every?;
    let open = open.map_err(|_| {
        GhFailure::network("gh open pull request checks reader failed".to_owned())
    })??;
    let mut pull_requests = parse_pull_requests(&every).map_err(GhFailure::network)?;
    let mut open_checks = parse_open_checks(&open).map_err(GhFailure::network)?;
    for pull_request in &mut pull_requests {
        let fresh = if is_open(pull_request) {
            open_checks.remove(&pull_request.number)
        } else {
            None
        };
        pull_request.checks = fresh
            .or_else(|| {
                let head = pull_request.head_oid.clone()?;
                known.get(&(pull_request.number, head)).copied()
            })
            .unwrap_or(PullRequestChecks::Unknown);
    }
    Ok(pull_requests)
}

/// The checks to keep for the next read: this answer's, or, when it never
/// got as far as a pull request list, the ones kept before it.
fn carried_checks(answer: &GithubProjectSnapshot, previous: KnownChecks) -> KnownChecks {
    if answer.pull_requests_read {
        remembered_checks(&answer.pull_requests)
    } else {
        previous
    }
}

/// Every check that was actually read, by number and head commit.
fn remembered_checks(pull_requests: &[PullRequestSnapshot]) -> KnownChecks {
    pull_requests
        .iter()
        .filter(|pull_request| pull_request.checks != PullRequestChecks::Unknown)
        .filter_map(|pull_request| {
            Some((
                (pull_request.number, pull_request.head_oid.clone()?),
                pull_request.checks,
            ))
        })
        .collect()
}

/// Optional Project fields may be inaccessible with otherwise valid issue
/// permissions. Retry exactly once without them, and retain the diagnostic.
fn with_optional_projects<T>(
    mut read: impl FnMut(bool) -> Result<T, GhFailure>,
) -> Result<(T, Option<GhFailure>), GhFailure> {
    match read(true) {
        Ok(value) => Ok((value, None)),
        Err(reason) => {
            crate::diagnostic!(
                serde_json::json!({"component":"github","kind":"project_metadata.unavailable","message":reason.reason})
            );
            read(false).map(|value| (value, Some(reason)))
        }
    }
}

fn read_issues(
    root: &Path,
    links: &[crate::issues::IssueReference],
) -> Result<(crate::issues::ProjectIssuesSnapshot, Option<GhFailure>), GhFailure> {
    let repository_json = gh(Some(root), &["repo", "view", "--json", "nameWithOwner"])?;
    let repository: serde_json::Value = serde_json::from_str(&repository_json)
        .map_err(|error| GhFailure::network(format!("GitHub repository response: {error}")))?;
    let repository = repository
        .get("nameWithOwner")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| GhFailure::network("GitHub repository identity is missing".into()))?
        .to_owned();
    crate::issues::IssueReference::parse(&format!("{repository}#1"), None)
        .map_err(GhFailure::network)?;
    // One sentinel proves overflow; ordinary gh list sorts by creation.
    let (listed, mut warning) = with_optional_projects(|include_projects| {
        let fields = if include_projects {
            "number,title,url,state,projectItems,updatedAt,createdAt,closedAt"
        } else {
            "number,title,url,state,updatedAt,createdAt,closedAt"
        };
        let output = gh(
            Some(root),
            &[
                "issue",
                "list",
                "--state",
                "open",
                "--limit",
                "201",
                "--search",
                "sort:updated-desc",
                "--json",
                fields,
            ],
        )?;
        crate::issues::parse_issues(&output).map_err(GhFailure::network)
    })?;
    let mut overflow = listed.len() > crate::issues::ISSUE_LIMIT;
    let mut issues = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let links: Vec<_> = links
        .iter()
        .filter(|reference| seen.insert((*reference).clone()))
        .take(crate::issues::ISSUE_LIMIT)
        .collect();
    let missing: Vec<_> = links
        .iter()
        .copied()
        .filter(|reference| !listed.iter().any(|issue| &issue.reference == *reference))
        .collect();
    // Closed and cross-repository links are read together in one bounded
    // GraphQL query, rather than spawning one subprocess per card.
    let linked = if missing.is_empty() {
        Vec::new()
    } else {
        let (linked, linked_warning) = read_linked_issues(root, &missing)?;
        if warning.is_none() {
            warning = linked_warning;
        }
        linked
    };
    for reference in links {
        if let Some(issue) = listed
            .iter()
            .chain(linked.iter())
            .find(|issue| &issue.reference == reference)
        {
            issues.push(issue.clone());
        }
    }
    for issue in listed {
        if issues
            .iter()
            .any(|known| known.reference == issue.reference)
        {
            continue;
        }
        if issues.len() == crate::issues::ISSUE_LIMIT {
            overflow = true;
            break;
        }
        issues.push(issue);
    }
    // Every kept issue's blockers in one bounded query, never one per card. A
    // failure keeps each issue's earlier blockers (`ingest_github`) and is
    // said beside the tasks, not in place of them.
    let dependencies_failure = match read_dependencies(root, &issues) {
        Ok(blockers) => {
            for issue in &mut issues {
                issue.blocked_by = blockers.get(&issue.reference).cloned().unwrap_or_default();
            }
            None
        }
        Err(reason) => {
            crate::diagnostic!(serde_json::json!({
                "component": "github",
                "kind": "issue_dependencies.unavailable",
                "category": reason.category,
                "message": reason.reason,
            }));
            Some(reason.reason)
        }
    };
    Ok((
        crate::issues::ProjectIssuesSnapshot {
            repository: Some(repository),
            issues,
            overflow,
            dependencies_failure,
        },
        warning,
    ))
}

/// How many blockers of one issue a read keeps; GitHub orders them, and an
/// issue blocked by more than this is drawn with the first ones.
const BLOCKERS_PER_ISSUE: usize = 20;

type Blockers =
    std::collections::BTreeMap<crate::issues::IssueReference, Vec<crate::issues::IssueReference>>;

/// The open issues blocking each of `issues`, from GitHub's issue
/// dependencies (`Issue.blockedBy`), read in one GraphQL query grouped by
/// repository. A closed blocker no longer blocks and is left out.
fn read_dependencies(
    root: &Path,
    issues: &[crate::issues::IssueSnapshot],
) -> Result<Blockers, GhFailure> {
    if issues.is_empty() {
        return Ok(Blockers::new());
    }
    let query = dependency_query(issues)?;
    let output = gh(
        Some(root),
        &["api", "graphql", "-f", &format!("query={query}")],
    )?;
    parse_dependencies(&output)
}

fn dependency_query(issues: &[crate::issues::IssueSnapshot]) -> Result<String, GhFailure> {
    let mut repositories: std::collections::BTreeMap<&str, Vec<u32>> =
        std::collections::BTreeMap::new();
    for issue in issues {
        repositories
            .entry(issue.reference.repository.as_str())
            .or_default()
            .push(issue.reference.number);
    }
    let mut query = String::from("query HideIssueDependencies {");
    for (index, (repository, numbers)) in repositories.iter().enumerate() {
        let valid = crate::issues::IssueReference::parse(&format!("{repository}#1"), None)
            .map_err(GhFailure::network)?;
        let (owner, name) = valid
            .repository
            .split_once('/')
            .expect("validated repository");
        query.push_str(&format!(
            "r{index}:repository(owner:\"{owner}\",name:\"{name}\"){{nameWithOwner"
        ));
        for number in numbers {
            query.push_str(&format!(" i{number}:issue(number:{number}){{number blockedBy(first:{BLOCKERS_PER_ISSUE}){{nodes{{number state repository{{nameWithOwner}}}}}}}}"));
        }
        query.push('}');
    }
    query.push('}');
    Ok(query)
}

fn parse_dependencies(output: &str) -> Result<Blockers, GhFailure> {
    let answer: serde_json::Value =
        serde_json::from_str(output).map_err(|error| GhFailure::network(error.to_string()))?;
    if answer.get("errors").is_some() {
        return Err(GhFailure::network(
            "GitHub could not read the issue dependencies".into(),
        ));
    }
    let data = answer
        .get("data")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| GhFailure::network("GitHub issue dependency response has no data".into()))?;
    let reference = |repository: &str, number: &serde_json::Value| {
        let number = number.as_u64().ok_or_else(|| {
            GhFailure::network("GitHub returned an issue without a number".into())
        })?;
        crate::issues::IssueReference::parse(&format!("{repository}#{number}"), None)
            .map_err(GhFailure::network)
    };
    let mut blockers = Blockers::new();
    for repository in data.values() {
        let Some(fields) = repository.as_object() else {
            continue;
        };
        let name = fields
            .get("nameWithOwner")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                GhFailure::network("GitHub dependency response names no repository".into())
            })?;
        for issue in fields.values().filter(|value| value.is_object()) {
            let blocked = reference(name, &issue["number"])?;
            let mut open = Vec::new();
            for node in issue
                .pointer("/blockedBy/nodes")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                if node["state"] != "OPEN" {
                    continue;
                }
                let owner = node
                    .pointer("/repository/nameWithOwner")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(name);
                open.push(reference(owner, &node["number"])?);
            }
            blockers.insert(blocked, open);
        }
    }
    Ok(blockers)
}

pub(crate) fn read_linked_issue(
    root: &Path,
    reference: &crate::issues::IssueReference,
) -> Result<crate::issues::IssueSnapshot, String> {
    read_linked_issues(root, &[reference])
        .map_err(|error| error.reason)?
        .0
        .into_iter()
        .next()
        .ok_or_else(|| "GitHub issue was not found".to_owned())
}

/// Creates an issue in the repository `root` belongs to (Overview › 새 이슈,
/// and a pull request's 새 이슈 만들기), one of Hide's two writes to GitHub
/// with `write_closing_line`. `gh issue create` prints the new issue's URL,
/// which names it.
pub(crate) fn create_issue(
    root: &Path,
    title: &str,
    body: &str,
) -> Result<crate::issues::IssueSnapshot, String> {
    let output = gh(
        Some(root),
        &["issue", "create", "--title", title, "--body", body],
    )
    .map_err(|error| error.reason)?;
    let url = output
        .lines()
        .map(str::trim)
        .rfind(|line| line.starts_with("https://github.com/"))
        .ok_or_else(|| {
            format!(
                "gh issue create did not print the new issue's URL: {}",
                output.trim()
            )
        })?;
    let reference = crate::issues::IssueReference::parse(url, None)?;
    Ok(crate::issues::IssueSnapshot {
        reference,
        title: title.to_owned(),
        url: url.to_owned(),
        state: "OPEN".into(),
        project_status: None,
        updated_at_unix_ms: Some(now_unix_ms()),
        created_at_unix_ms: None,
        closed_at_unix_ms: None,
        blocked_by: Vec::new(),
    })
}

/// The fields `issue_detail` asks `gh issue view` for, and the only ones
/// `run_gh` lets it ask for.
const ISSUE_DETAIL_FIELDS: &str = "body,labels,author,assignees,comments,createdAt";

/// One issue as its panel reads it when it opens (PRD overview-lenses-issues
/// D-40), and the Start dialog's first prompt: the body, labels, author,
/// assignees and the latest comments, in one `gh issue view` on the caller's
/// worker.
pub(crate) fn issue_detail(
    root: &Path,
    reference: &crate::issues::IssueReference,
) -> Result<crate::tasks::TaskDetail, String> {
    let number = reference.number.to_string();
    let output = gh(
        Some(root),
        &[
            "issue",
            "view",
            &number,
            "--repo",
            &reference.repository,
            "--json",
            ISSUE_DETAIL_FIELDS,
        ],
    )
    .map_err(|error| error.reason)?;
    parse_issue_detail(&output)
}

fn parse_issue_detail(output: &str) -> Result<crate::tasks::TaskDetail, String> {
    #[derive(serde::Deserialize)]
    struct Person {
        login: String,
    }
    #[derive(serde::Deserialize)]
    struct Label {
        name: String,
        #[serde(default)]
        color: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct Comment {
        #[serde(default)]
        author: Option<Person>,
        #[serde(default, rename = "createdAt")]
        created_at: Option<String>,
        #[serde(default)]
        body: String,
    }
    #[derive(serde::Deserialize)]
    struct Viewed {
        body: String,
        #[serde(default)]
        labels: Vec<Label>,
        #[serde(default)]
        author: Option<Person>,
        #[serde(default)]
        assignees: Vec<Person>,
        #[serde(default)]
        comments: Vec<Comment>,
        #[serde(default, rename = "createdAt")]
        created_at: Option<String>,
    }
    let viewed = serde_json::from_str::<Viewed>(output)
        .map_err(|error| format!("gh issue view returned output Hide could not read: {error}"))?;
    let count = viewed.comments.len();
    let latest = count.saturating_sub(crate::tasks::DETAIL_COMMENTS);
    Ok(crate::tasks::TaskDetail {
        body: viewed.body,
        labels: viewed
            .labels
            .into_iter()
            .map(|label| crate::tasks::TaskLabel {
                name: label.name,
                // A colour is drawn from data, so only six hex digits pass.
                color: label.color.filter(|color| {
                    color.len() == 6 && color.bytes().all(|byte| byte.is_ascii_hexdigit())
                }),
            })
            .collect(),
        author: viewed.author.map(|person| person.login),
        created_at_unix_ms: viewed.created_at.as_deref().and_then(parse_rfc3339_ms),
        assignees: viewed
            .assignees
            .into_iter()
            .map(|person| person.login)
            .collect(),
        comment_count: Some(u32::try_from(count).unwrap_or(u32::MAX)),
        comments: viewed
            .comments
            .into_iter()
            .skip(latest)
            .map(|comment| crate::tasks::TaskComment {
                author: comment.author.map(|person| person.login),
                created_at_unix_ms: comment.created_at.as_deref().and_then(parse_rfc3339_ms),
                body: crate::tasks::capped_comment(&comment.body),
            })
            .collect(),
    })
}

/// The fields `search` asks `gh search prs` and `gh search issues` for, and
/// the only ones `run_gh` lets them ask for. The search has no head branch, and
/// only a pull request has `isDraft`.
const SEARCH_PR_FIELDS: &str = "isDraft,number,repository,state,title,url";
const SEARCH_ISSUE_FIELDS: &str = "number,repository,state,title,url";

/// Most pull requests, and most issues, one search returns in all, and what
/// one `gh search` is asked for per repository.
pub(crate) const SEARCH_LIMIT: usize = 20;

/// Longest query, in characters, a search takes.
pub(crate) const SEARCH_QUERY_LIMIT: usize = 200;

/// Most repositories one search names (`runtime/issues.rs` caps the projects at the same number).
pub(crate) const SEARCH_REPOSITORY_LIMIT: usize = 20;

/// A query as `run_gh` lets it reach `gh`: some text, within the cap.
fn is_search_query(query: &str) -> bool {
    !query.trim().is_empty() && query.chars().count() <= SEARCH_QUERY_LIMIT
}

/// The one shape of `gh search prs|issues` Hide runs: `search <kind>`, one
/// `--repo owner/name` per repository, the cap, that kind's fixed fields, then
/// `--` and the query's words, each of which is just text to `gh`.
fn is_search_call(arguments: &[&str]) -> bool {
    let [first, kind, rest @ ..] = arguments else {
        return false;
    };
    let fields = match (*first, *kind) {
        ("search", "prs") => SEARCH_PR_FIELDS,
        ("search", "issues") => SEARCH_ISSUE_FIELDS,
        _ => return false,
    };
    let mut rest = rest;
    let mut repositories = 0;
    while let ["--repo", repository, tail @ ..] = rest {
        if !crate::issues::is_repository(repository) {
            return false;
        }
        repositories += 1;
        rest = tail;
    }
    let limit = SEARCH_LIMIT.to_string();
    let ["--limit", count, "--json", requested, "--", words @ ..] = rest else {
        return false;
    };
    (1..=SEARCH_REPOSITORY_LIMIT).contains(&repositories)
        && *count == limit
        && *requested == fields
        && !words.is_empty()
        && words.iter().map(|word| word.chars().count()).sum::<usize>() <= SEARCH_QUERY_LIMIT
        && words.iter().all(|word| is_search_query(word))
}

#[derive(Clone, Copy)]
enum SearchKind {
    Pr,
    Issue,
}

impl SearchKind {
    fn subcommand(self) -> &'static str {
        match self {
            Self::Pr => "prs",
            Self::Issue => "issues",
        }
    }

    fn fields(self) -> &'static str {
        match self {
            Self::Pr => SEARCH_PR_FIELDS,
            Self::Issue => SEARCH_ISSUE_FIELDS,
        }
    }

    fn wire(self) -> &'static str {
        match self {
            Self::Pr => "pr",
            Self::Issue => "issue",
        }
    }
}

/// A project a search may cover: where it lives, and its `owner/name` when
/// the GitHub reader has already resolved it.
#[derive(Clone, Debug)]
pub(crate) struct SearchTarget {
    pub(crate) root: std::path::PathBuf,
    pub(crate) repository: Option<String>,
}

/// Searches pull requests and issues for `query` in each target's
/// repository, on the caller's worker. A project the reader has not read yet
/// is resolved here with the same `gh repo view` the reader runs, so the
/// search never depends on what was read earlier. See `search_with`.
pub(crate) fn search(
    targets: &[SearchTarget],
    query: &str,
) -> Result<Vec<crate::model::GithubSearchResult>, String> {
    let repositories = resolve_repositories(gh, targets)?;
    search_with(|arguments| gh(None, arguments), &repositories, query)
}

/// The targets' repositories, once each by `owner/name` (GitHub names ignore
/// case), in order. A project with no GitHub remote is left out; a project
/// whose repository could not be read for any other reason fails the search,
/// which then cannot say it covered every project.
fn resolve_repositories(
    run: impl Fn(Option<&Path>, &[&str]) -> Result<String, GhFailure>,
    targets: &[SearchTarget],
) -> Result<Vec<String>, String> {
    let mut repositories: Vec<String> = Vec::new();
    for target in targets {
        let repository = match &target.repository {
            Some(repository) => repository.clone(),
            None => match run(
                Some(&target.root),
                &["repo", "view", "--json", "nameWithOwner"],
            )
            .and_then(|output| {
                serde_json::from_str::<serde_json::Value>(&output)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("nameWithOwner")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    })
                    .filter(|name| crate::issues::is_repository(name))
                    .ok_or_else(|| GhFailure::network("repository identity is missing".into()))
            }) {
                Ok(repository) => repository,
                // A project with no GitHub remote is not one to search; any
                // other failure means the search cannot say it covered it.
                Err(failure) if failure.category == "no GitHub remote" => continue,
                Err(failure) => {
                    return Err(format!(
                        "gh repo view in {}: {}",
                        target.root.display(),
                        failure.reason
                    ));
                }
            },
        };
        if !repositories
            .iter()
            .any(|known| known.eq_ignore_ascii_case(&repository))
        {
            repositories.push(repository);
        }
    }
    Ok(repositories)
}

/// Searches pull requests and issues for `query` in all the repositories at
/// once: one `gh search prs` and one `gh search issues`, however many
/// repositories there are (GitHub's search allows about 30 calls a minute).
/// The query goes as separate words, so it matches the way a search box does,
/// not as one quoted phrase. At most `SEARCH_LIMIT` of each come back, pull
/// requests first. A failed call fails the whole search, naming the call.
fn search_with(
    run: impl Fn(&[&str]) -> Result<String, GhFailure>,
    repositories: &[String],
    query: &str,
) -> Result<Vec<crate::model::GithubSearchResult>, String> {
    if repositories.is_empty() {
        return Ok(Vec::new());
    }
    let limit = SEARCH_LIMIT.to_string();
    let mut found: Vec<Vec<crate::model::GithubSearchResult>> = Vec::new();
    for kind in [SearchKind::Pr, SearchKind::Issue] {
        let mut arguments = vec!["search", kind.subcommand()];
        for repository in repositories {
            arguments.extend(["--repo", repository]);
        }
        arguments.extend(["--limit", &limit, "--json", kind.fields(), "--"]);
        arguments.extend(query.split_whitespace());
        let output = run(&arguments)
            .map_err(|error| format!("gh search {}: {}", kind.subcommand(), error.reason))?;
        let mut hits = parse_search(&output, kind, repositories)?;
        hits.truncate(SEARCH_LIMIT);
        found.push(hits);
    }
    Ok(found.into_iter().flatten().collect())
}

/// One `gh search` answer. A hit outside `repositories` is dropped: a
/// `repo:` qualifier in the query widens GitHub's search to that repository
/// too, and a search only covers the projects registered here.
fn parse_search(
    output: &str,
    kind: SearchKind,
    repositories: &[String],
) -> Result<Vec<crate::model::GithubSearchResult>, String> {
    #[derive(Deserialize)]
    struct Found {
        number: u32,
        title: String,
        state: String,
        url: String,
        #[serde(default, rename = "isDraft")]
        is_draft: bool,
        repository: FoundRepository,
    }
    #[derive(Deserialize)]
    struct FoundRepository {
        #[serde(rename = "nameWithOwner")]
        name_with_owner: String,
    }
    let found = serde_json::from_str::<Vec<Found>>(output).map_err(|error| {
        format!(
            "gh search {} returned output Hide could not read: {error}",
            kind.subcommand()
        )
    })?;
    let mut results = Vec::new();
    for hit in found {
        let repository = hit.repository.name_with_owner;
        if !repositories
            .iter()
            .any(|known| known.eq_ignore_ascii_case(&repository))
        {
            continue;
        }
        let state = hit.state.to_ascii_lowercase();
        if !matches!(state.as_str(), "open" | "closed" | "merged") {
            return Err(format!(
                "gh search {} returned the state {:?}",
                kind.subcommand(),
                hit.state
            ));
        }
        // The link is drawn from data, so only a github.com address passes.
        if !hit.url.starts_with("https://github.com/") {
            return Err(format!(
                "gh search {} returned a link that is not on github.com",
                kind.subcommand()
            ));
        }
        results.push(crate::model::GithubSearchResult {
            kind: kind.wire().into(),
            repository,
            number: hit.number,
            title: hit.title,
            state,
            url: hit.url,
            is_draft: matches!(kind, SearchKind::Pr) && hit.is_draft,
        });
    }
    Ok(results)
}

/// The fields `pr_feedback` asks `gh pr view` for, and the only ones
/// `run_gh` lets it ask for.
const PR_FEEDBACK_FIELDS: &str = "body,statusCheckRollup,reviews";

/// What a pull request says for the work handed on from it (PRD
/// overview-lenses-prs): its body, which a new issue made from it starts
/// with (B12), and its failed checks and the reviews still asking for
/// changes, which an agent it is handed to starts from (D-46).
#[derive(Debug)]
pub(crate) struct PrFeedback {
    pub(crate) body: String,
    pub(crate) failed_checks: Vec<crate::model::PrFailedCheck>,
    pub(crate) change_requests: Vec<crate::model::PrChangeRequest>,
}

/// `PrFeedback`, in one `gh pr view` on the caller's worker.
pub(crate) fn pr_feedback(root: &Path, number: u32) -> Result<PrFeedback, String> {
    let number = number.to_string();
    let output = gh(
        Some(root),
        &["pr", "view", &number, "--json", PR_FEEDBACK_FIELDS],
    )
    .map_err(|error| error.reason)?;
    parse_pr_feedback(&output)
}

fn parse_pr_feedback(output: &str) -> Result<PrFeedback, String> {
    #[derive(serde::Deserialize)]
    struct Person {
        login: String,
    }
    #[derive(serde::Deserialize)]
    struct Review {
        #[serde(default)]
        author: Option<Person>,
        #[serde(default)]
        state: String,
        #[serde(default)]
        body: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Viewed {
        // All three are what was asked for: an answer without them is no
        // answer, never a pull request with nothing failing.
        body: String,
        status_check_rollup: Vec<GhCheck>,
        reviews: Vec<Review>,
    }
    let viewed = serde_json::from_str::<Viewed>(output)
        .map_err(|error| format!("gh pr view returned output Hide could not read: {error}"))?;
    let failed = viewed
        .status_check_rollup
        .iter()
        .filter_map(GhCheck::failure)
        .collect();
    // A reviewer's change request stands until that reviewer's next review
    // approves or dismisses it; a comment-only review leaves it standing, as
    // GitHub's own review decision reads it. Reviews come oldest first.
    let mut standing: Vec<(Option<String>, Review)> = Vec::new();
    for review in viewed.reviews {
        let author = review.author.as_ref().map(|person| person.login.clone());
        match review.state.as_str() {
            "CHANGES_REQUESTED" => {
                standing.retain(|(known, _)| *known != author);
                standing.push((author, review));
            }
            "APPROVED" | "DISMISSED" => standing.retain(|(known, _)| *known != author),
            _ => {}
        }
    }
    let requests = standing
        .into_iter()
        .map(|(author, review)| crate::model::PrChangeRequest {
            author,
            body: review.body.trim().to_owned(),
        })
        .collect();
    Ok(PrFeedback {
        body: viewed.body,
        failed_checks: failed,
        change_requests: requests,
    })
}

/// Writes `Closes #issue` at the end of pull request `number`'s body so
/// GitHub closes the issue when it merges (PRD overview-lenses-prs D-13,
/// D-47): the body is read first, and a body that already closes the issue
/// is left as it is, so a retry never adds a second line. This is Hide's one
/// write to a pull request.
pub(crate) fn write_closing_line(root: &Path, number: u32, issue: u32) -> Result<bool, String> {
    write_closing_line_with(|arguments| gh(Some(root), arguments), number, issue)
}

/// `write_closing_line` through `gh`, which a test answers with a fixture.
fn write_closing_line_with(
    gh: impl Fn(&[&str]) -> Result<String, GhFailure>,
    number: u32,
    issue: u32,
) -> Result<bool, String> {
    let number = number.to_string();
    let output = gh(&["pr", "view", &number, "--json", "body"]).map_err(|error| error.reason)?;
    #[derive(serde::Deserialize)]
    struct Viewed {
        #[serde(default)]
        body: String,
    }
    let body = serde_json::from_str::<Viewed>(&output)
        .map_err(|error| format!("gh pr view returned output Hide could not read: {error}"))?
        .body;
    if closes_issue(&body, issue) {
        return Ok(false);
    }
    let next = with_closing_line(&body, issue);
    gh(&["pr", "edit", &number, "--body", &next]).map_err(|error| error.reason)?;
    Ok(true)
}

/// The keywords GitHub reads as closing an issue from a pull request's body.
const CLOSING_KEYWORDS: [&str; 9] = [
    "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
];

/// Whether `body` already closes issue `issue` of the pull request's own
/// repository: a closing keyword followed by `#N`, `owner/repo#N` or the
/// issue's URL. GitHub's own rule is prose, so this reads it word by word at
/// this one boundary.
pub(crate) fn closes_issue(body: &str, issue: u32) -> bool {
    let words: Vec<&str> = body.split_whitespace().collect();
    words.windows(2).any(|pair| {
        let keyword = pair[0].trim_end_matches(':').to_ascii_lowercase();
        CLOSING_KEYWORDS.contains(&keyword.as_str()) && referenced_issue(pair[1]) == Some(issue)
    })
}

/// The issue number a word names: `#12`, `owner/repo#12` or
/// `https://github.com/owner/repo/issues/12`, trailing punctuation aside.
fn referenced_issue(word: &str) -> Option<u32> {
    let word = word.trim_end_matches(['.', ',', ';', ':', ')', '!', '?']);
    let number = if let Some(rest) = word.strip_prefix("https://github.com/") {
        rest.split_once("/issues/")?.1
    } else {
        let (repository, number) = word.rsplit_once('#')?;
        if !(repository.is_empty() || repository.split('/').count() == 2) {
            return None;
        }
        number
    };
    if !is_number(number) {
        return None;
    }
    number.parse().ok()
}

/// `body` with `Closes #issue` on its own line after a blank line.
fn with_closing_line(body: &str, issue: u32) -> String {
    let body = body.trim_end();
    if body.is_empty() {
        format!("Closes #{issue}")
    } else {
        format!("{body}\n\nCloses #{issue}")
    }
}

fn read_linked_issues(
    root: &Path,
    links: &[&crate::issues::IssueReference],
) -> Result<(Vec<crate::issues::IssueSnapshot>, Option<GhFailure>), GhFailure> {
    with_optional_projects(|include_projects| {
        let query = issue_query(links, include_projects)?;
        let output = gh(
            Some(root),
            &["api", "graphql", "-f", &format!("query={query}")],
        )?;
        parse_linked_issues(&output)
    })
}

fn issue_query(
    links: &[&crate::issues::IssueReference],
    include_projects: bool,
) -> Result<String, GhFailure> {
    let mut query = String::from("query HideLinkedIssues {");
    let projects = if include_projects {
        " projectItems(first:100){nodes{status:fieldValueByName(name:\"Status\"){... on ProjectV2ItemFieldSingleSelectValue{name}}}}"
    } else {
        ""
    };
    for (index, reference) in links.iter().enumerate() {
        let valid = crate::issues::IssueReference::parse(&reference.token(), None)
            .map_err(GhFailure::network)?;
        let (owner, name) = valid
            .repository
            .split_once('/')
            .expect("validated repository");
        query.push_str(&format!("r{index}:repository(owner:\"{owner}\",name:\"{name}\"){{issue(number:{}){{number title url state updatedAt createdAt closedAt{projects}}}}}", valid.number));
    }
    query.push('}');
    Ok(query)
}

fn parse_linked_issues(output: &str) -> Result<Vec<crate::issues::IssueSnapshot>, GhFailure> {
    let answer: serde_json::Value =
        serde_json::from_str(output).map_err(|error| GhFailure::network(error.to_string()))?;
    if answer.get("errors").is_some() {
        return Err(GhFailure::network(
            "GitHub could not resolve the linked issues".into(),
        ));
    }
    let data = answer
        .get("data")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| GhFailure::network("GitHub linked issue response has no data".into()))?;
    let mut issues = Vec::new();
    for repository in data.values() {
        let Some(mut issue) = repository
            .get("issue")
            .filter(|issue| !issue.is_null())
            .cloned()
        else {
            crate::diagnostic!(
                serde_json::json!({"component":"github","kind":"linked_issue.unavailable"})
            );
            continue;
        };
        let items = issue
            .pointer("/projectItems/nodes")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        issue["projectItems"] = items;
        issues.push(issue);
    }
    crate::issues::parse_issues(&serde_json::Value::Array(issues).to_string())
        .map_err(GhFailure::network)
}

fn failed(reason: GhFailure) -> GithubStatusSnapshot {
    GithubStatusSnapshot {
        available: true,
        loading: false,
        stale: true,
        last_success_at_unix_ms: None,
        unavailable_reason: Some(reason.reason),
        failure_category: Some(reason.category.to_owned()),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPullRequest {
    title: String,
    number: u32,
    head_ref_name: String,
    #[serde(default)]
    head_ref_oid: Option<String>,
    #[serde(default)]
    is_cross_repository: bool,
    base_ref_name: String,
    state: String,
    review_decision: Option<String>,
    is_draft: bool,
    url: String,
    merged_at: Option<String>,
    updated_at: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    closed_at: Option<String>,
    #[serde(default)]
    closing_issues_references: Vec<GhIssueReference>,
}

/// One row of the open list: a number and its checks, nothing else.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhOpenChecks {
    number: u32,
    status_check_rollup: Option<Vec<GhCheck>>,
}

#[derive(Debug, Deserialize)]
struct GhIssueReference {
    url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MergedPullRequestProof {
    pub head_oid: String,
    /// The branch the pull request merged into. Only `main` proves anything
    /// about main; a stacked pull request merged into its parent branch does not.
    pub base_ref: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhMergedPullRequestProof {
    head_ref_oid: String,
    base_ref_name: String,
}

/// Exact head commits and base branches of the merged pull requests on one
/// branch.
///
/// Cleanup uses this only when ordinary Git ancestry cannot prove that a
/// worktree is merged. A branch name alone is never deletion evidence: the
/// caller matches the head commit and the base.
pub(crate) fn merged_pull_request_proofs(
    root: &Path,
    branch: &str,
) -> Result<Vec<MergedPullRequestProof>, String> {
    let listed = gh(
        Some(root),
        &[
            "pr",
            "list",
            "--state",
            "merged",
            "--head",
            branch,
            "--limit",
            "100",
            "--json",
            "headRefOid,baseRefName",
        ],
    )
    .map_err(|failure| {
        format!(
            "GitHub merge proof is unavailable ({}): {}",
            failure.category, failure.reason
        )
    })?;
    parse_merged_pull_request_proofs(&listed)
}

fn parse_merged_pull_request_proofs(output: &str) -> Result<Vec<MergedPullRequestProof>, String> {
    let listed: Vec<GhMergedPullRequestProof> = serde_json::from_str(output)
        .map_err(|error| format!("gh pr list returned merge proof Hide could not read: {error}"))?;
    Ok(listed
        .into_iter()
        .map(|proof| MergedPullRequestProof {
            head_oid: proof.head_ref_oid,
            base_ref: proof.base_ref_name,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "__typename")]
enum GhCheck {
    CheckRun {
        status: String,
        conclusion: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default, rename = "detailsUrl")]
        details_url: Option<String>,
    },
    StatusContext {
        state: String,
        #[serde(default)]
        context: Option<String>,
        #[serde(default, rename = "targetUrl")]
        target_url: Option<String>,
    },
    #[serde(other)]
    Unknown,
}

/// A finished check run that did not pass.
const FAILED_CONCLUSIONS: [&str; 6] = [
    "FAILURE",
    "TIMED_OUT",
    "CANCELLED",
    "ACTION_REQUIRED",
    "STARTUP_FAILURE",
    "STALE",
];

/// A commit status that did not pass.
const FAILED_STATES: [&str; 2] = ["FAILURE", "ERROR"];

impl GhCheck {
    /// The check by the name GitHub shows and the page that says why, when
    /// it failed; `None` for any other check.
    fn failure(&self) -> Option<crate::model::PrFailedCheck> {
        let (name, url) = match self {
            GhCheck::CheckRun {
                status,
                conclusion,
                name,
                details_url,
            } if status == "COMPLETED"
                && conclusion
                    .as_deref()
                    .is_some_and(|value| FAILED_CONCLUSIONS.contains(&value)) =>
            {
                (name, details_url)
            }
            GhCheck::StatusContext {
                state,
                context,
                target_url,
            } if FAILED_STATES.contains(&state.as_str()) => (context, target_url),
            _ => return None,
        };
        Some(crate::model::PrFailedCheck {
            name: name.clone().unwrap_or_else(|| "이름 없는 검사".to_owned()),
            url: url.clone().filter(|url| url.starts_with("https://")),
        })
    }
}

fn rollup_checks(checks: Option<&[GhCheck]>) -> PullRequestChecks {
    let Some(checks) = checks else {
        return PullRequestChecks::Unknown;
    };
    if checks.is_empty() {
        return PullRequestChecks::None;
    }
    let mut pending = false;
    let mut unknown = false;
    for check in checks {
        match check {
            GhCheck::CheckRun {
                status, conclusion, ..
            } if status == "COMPLETED" => match conclusion.as_deref() {
                Some("SUCCESS" | "NEUTRAL" | "SKIPPED") => {}
                Some(value) if FAILED_CONCLUSIONS.contains(&value) => {
                    return PullRequestChecks::Failed;
                }
                _ => unknown = true,
            },
            GhCheck::CheckRun { status, .. }
                if matches!(
                    status.as_str(),
                    "QUEUED" | "IN_PROGRESS" | "WAITING" | "PENDING" | "REQUESTED"
                ) =>
            {
                pending = true
            }
            GhCheck::StatusContext { state, .. } => match state.as_str() {
                "SUCCESS" => {}
                value if FAILED_STATES.contains(&value) => return PullRequestChecks::Failed,
                "PENDING" | "EXPECTED" => pending = true,
                _ => unknown = true,
            },
            _ => unknown = true,
        }
    }
    if pending {
        PullRequestChecks::Pending
    } else if unknown {
        PullRequestChecks::Unknown
    } else {
        PullRequestChecks::Passing
    }
}

/// Every pull request `gh pr list --json` named, checks not read yet.
///
/// None is dropped: which one belongs to a checkout depends on the checkout's
/// commit, so a branch's older pull requests are candidates too
/// (`pull_request_for_checkout`).
pub fn parse_pull_requests(output: &str) -> Result<Vec<PullRequestSnapshot>, String> {
    let listed: Vec<GhPullRequest> = serde_json::from_str(output)
        .map_err(|error| format!("gh pr list returned output Hide could not read: {error}"))?;
    listed.into_iter().map(project).collect()
}

/// The open list's rows as checks by pull request number. A row whose rollup
/// is absent is `Unknown`, as it always was.
fn parse_open_checks(output: &str) -> Result<HashMap<u32, PullRequestChecks>, String> {
    let listed: Vec<GhOpenChecks> = serde_json::from_str(output)
        .map_err(|error| format!("gh pr list returned checks Hide could not read: {error}"))?;
    Ok(listed
        .into_iter()
        .map(|row| {
            (
                row.number,
                rollup_checks(row.status_check_rollup.as_deref()),
            )
        })
        .collect())
}

fn project(listed: GhPullRequest) -> Result<PullRequestSnapshot, String> {
    let review = review_decision(listed.review_decision.as_deref());
    Ok(PullRequestSnapshot {
        closing_issues: listed
            .closing_issues_references
            .into_iter()
            .map(|issue| crate::issues::IssueReference::parse(&issue.url, None))
            .collect::<Result<_, _>>()?,
        title: listed.title,
        checks: PullRequestChecks::Unknown,
        badge: badge(&listed.state, listed.is_draft, review),
        number: listed.number,
        head_branch: listed.head_ref_name,
        base_branch: listed.base_ref_name,
        url: listed.url,
        review: review.filter(|_| !listed.is_draft && listed.state.eq_ignore_ascii_case("OPEN")),
        is_draft: listed.is_draft,
        merged_at_unix_ms: listed.merged_at.as_deref().and_then(parse_rfc3339_ms),
        updated_at_unix_ms: listed.updated_at.as_deref().and_then(parse_rfc3339_ms),
        created_at_unix_ms: listed.created_at.as_deref().and_then(parse_rfc3339_ms),
        closed_at_unix_ms: listed.closed_at.as_deref().and_then(parse_rfc3339_ms),
        head_oid: listed.head_ref_oid.filter(|oid| !oid.is_empty()),
        cross_repository: listed.is_cross_repository,
    })
}

/// The mapping the whole feature's colour scheme rests on.
///
/// A draft is `open` whatever its review decision says, because a draft is not
/// asking to be reviewed. Anything `gh` reports that is not one of the three
/// known states is `open` rather than being dropped: a branch with a pull
/// request must not present as a branch with none.
pub fn badge(state: &str, is_draft: bool, review: Option<ReviewDecision>) -> PullRequestBadge {
    if state.eq_ignore_ascii_case("MERGED") {
        return PullRequestBadge::Merged;
    }
    if state.eq_ignore_ascii_case("CLOSED") {
        return PullRequestBadge::Closed;
    }
    if !is_draft && review.is_some() {
        return PullRequestBadge::Review;
    }
    PullRequestBadge::Open
}

pub fn review_decision(value: Option<&str>) -> Option<ReviewDecision> {
    match value?.to_ascii_uppercase().as_str() {
        "REVIEW_REQUIRED" => Some(ReviewDecision::ReviewRequired),
        "CHANGES_REQUESTED" => Some(ReviewDecision::ChangesRequested),
        "APPROVED" => Some(ReviewDecision::Approved),
        // An empty string is what `gh` reports for a pull request nobody has
        // been asked to review, which is `open`, not an unknown decision.
        _ => None,
    }
}

/// Whether `pull_request` is the work of a checkout on `branch` at commit
/// `head`. Every place a pull request is tied to a checkout asks this and
/// nothing else, because a branch name is not an identity: it is used again.
///
/// - An open pull request belongs to the checkout on its head branch, unless
///   that branch lives in a fork, whose name is only a coincidence.
/// - A merged or closed one belongs only while the checkout's commit is
///   exactly the pull request's head. A branch name reused for new work has
///   a new commit, and the old merge must not follow it.
///
/// A checkout whose commit is not known yet (`head` absent) takes no settled
/// pull request, since it cannot be told from the reused name.
pub(crate) fn belongs_to_checkout(
    pull_request: &PullRequestSnapshot,
    branch: Option<&str>,
    head: Option<&str>,
) -> bool {
    if branch != Some(pull_request.head_branch.as_str()) {
        return false;
    }
    if is_open(pull_request) {
        return !pull_request.cross_repository;
    }
    matches!((pull_request.head_oid.as_deref(), head), (Some(pull), Some(head)) if pull == head)
}

/// The pull request a checkout shows when several qualify: an open one, then
/// a merged one, then a closed one, the most recently updated first among
/// equals. A branch merged and then reopened for more work shows the work in
/// flight, not the merge behind it.
pub(crate) fn pull_request_for_checkout<'a>(
    pull_requests: &'a [PullRequestSnapshot],
    branch: Option<&str>,
    head: Option<&str>,
) -> Option<&'a PullRequestSnapshot> {
    pull_requests
        .iter()
        .filter(|pull_request| belongs_to_checkout(pull_request, branch, head))
        .min_by(|left, right| precedence(left, right))
}

fn precedence(left: &PullRequestSnapshot, right: &PullRequestSnapshot) -> std::cmp::Ordering {
    let rank = |pull_request: &PullRequestSnapshot| match pull_request.badge {
        PullRequestBadge::Merged => 1,
        PullRequestBadge::Closed => 2,
        PullRequestBadge::Open | PullRequestBadge::Review => 0,
    };
    rank(left)
        .cmp(&rank(right))
        .then_with(|| right.updated_at_unix_ms.cmp(&left.updated_at_unix_ms))
        .then_with(|| right.number.cmp(&left.number))
}

/// One pull request per head branch, for the views that list a branch once
/// (the PRs view, a worktree's base): the one `precedence` puts first,
/// whatever the checkout's commit. Which pull request a checkout holds is
/// `pull_request_for_checkout`'s answer, never this one.
pub(crate) fn preferred_per_branch(
    pull_requests: &[PullRequestSnapshot],
) -> Vec<&PullRequestSnapshot> {
    let mut candidates: Vec<&PullRequestSnapshot> = pull_requests.iter().collect();
    candidates.sort_by(|left, right| {
        left.head_branch
            .cmp(&right.head_branch)
            .then_with(|| precedence(left, right))
    });
    candidates.dedup_by(|later, kept| later.head_branch == kept.head_branch);
    candidates
}

fn is_open(pull_request: &PullRequestSnapshot) -> bool {
    !pull_request.badge.is_settled()
}

/// `gh` timestamps are RFC 3339 in UTC (`2026-09-03T04:15:00Z`). Only the
/// instant is needed - the card renders "N minutes ago" from it - so this
/// converts without pulling in a date library for one format.
pub fn parse_rfc3339_ms(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.len() < 20 {
        return None;
    }
    let bytes = value.as_bytes();
    let number = |start: usize, end: usize| -> Option<u64> {
        std::str::from_utf8(&bytes[start..end]).ok()?.parse().ok()
    };
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second) * 1_000)
}

/// Days since 1970-01-01 for a proleptic Gregorian date at or after it.
///
/// Written as whole leap-year counts rather than an era formula: the inputs
/// are `gh` timestamps, all of them modern, and a reader can check this by
/// eye against a calendar.
fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    const CUMULATIVE: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap_days_before = |year: u64| {
        let previous = year - 1;
        previous / 4 - previous / 100 + previous / 400
    };
    let leap = |year: u64| {
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
    };
    let leaps = leap_days_before(year) - leap_days_before(1970);
    let leap_day_this_year = u64::from(leap(year) && month > 2);
    365 * (year - 1970) + leaps + CUMULATIVE[(month - 1) as usize] + leap_day_this_year + day - 1
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn main_worktree(path: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let common = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if common.is_empty() {
        return None;
    }
    PathBuf::from(common).parent().map(Path::to_path_buf)
}

#[derive(Clone, Debug)]
struct GhFailure {
    category: &'static str,
    reason: String,
}

impl GhFailure {
    fn network(reason: String) -> Self {
        Self {
            category: "network or rate limit",
            reason,
        }
    }
}

// gh exposes these failures only as human-readable stderr. Keep the classifier
// at this external boundary and preserve unknown errors verbatim as network failures.
fn classify_failure(reason: String, exit_code: Option<i32>) -> GhFailure {
    let lower = reason.to_ascii_lowercase();
    let category = if exit_code == Some(4)
        || lower.contains("not logged")
        || lower.contains("gh auth login")
        || lower.contains("token") && lower.contains("invalid")
    {
        "not logged in"
    } else if lower.contains("no git remotes")
        || lower.contains("none of the git remotes")
        || lower.contains("not a github repository")
        || lower.contains("no github remote")
    {
        "no GitHub remote"
    } else {
        "network or rate limit"
    };
    GhFailure { category, reason }
}

fn gh(cwd: Option<&Path>, arguments: &[&str]) -> Result<String, GhFailure> {
    run_gh(Path::new("gh"), cwd, arguments, COMMAND_TIMEOUT)
}

/// A pull request or issue number as `gh` takes it.
fn is_number(argument: &str) -> bool {
    !argument.is_empty() && argument.bytes().all(|byte| byte.is_ascii_digit())
}

fn run_gh(
    binary: &Path,
    cwd: Option<&Path>,
    arguments: &[&str],
    timeout: Duration,
) -> Result<String, GhFailure> {
    if !(arguments.starts_with(&["auth", "status"])
        || arguments.starts_with(&["pr", "list"])
        || arguments.starts_with(&["issue", "list"])
        || arguments == ["repo", "view", "--json", "nameWithOwner"]
        // The two writes (docs/ARCHITECTURE.md): a new issue with a title and
        // a body, and a pull request's body, nothing else of either.
        || (arguments.len() == 6
            && arguments[..3] == ["issue", "create", "--title"]
            && arguments[4] == "--body")
        || (arguments.len() == 5
            && arguments[..2] == ["pr", "edit"]
            && is_number(arguments[2])
            && arguments[3] == "--body")
        || (arguments.len() == 5
            && arguments[..2] == ["pr", "view"]
            && is_number(arguments[2])
            && arguments[3] == "--json"
            && (arguments[4] == "body" || arguments[4] == PR_FEEDBACK_FIELDS))
        || (arguments.len() == 7
            && arguments[..2] == ["issue", "view"]
            && arguments[3] == "--repo"
            && arguments[5..] == ["--json", ISSUE_DETAIL_FIELDS])
        // The search (`search`): the registered repositories, the cap, fixed
        // fields, and the query's words after `--` so none can be read as a flag.
        || is_search_call(arguments)
        || (arguments.len() == 4
            && arguments[..3] == ["api", "graphql", "-f"]
            && (arguments[3].starts_with("query=query HideLinkedIssues {")
                || arguments[3].starts_with("query=query HideIssueDependencies {"))))
    {
        return Err(GhFailure::network("Unsupported gh command".to_owned()));
    }
    let mut command = Command::new(binary);
    command
        .args(arguments)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GH_PAGER", "cat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let mut child = OwnedChild::spawn(&mut command).map_err(|error| GhFailure {
        category: if error.kind() == std::io::ErrorKind::NotFound {
            "not installed"
        } else {
            "network or rate limit"
        },
        reason: format!("gh could not be run: {error}"),
    })?;
    // Drain both pipes while waiting, otherwise a large PR list fills stdout
    // and the child cannot exit before the timeout.
    let mut stdout = child.take_stdout().expect("piped stdout");
    let mut stderr = child.take_stderr().expect("piped stderr");
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10))
            }
            outcome => {
                let reason = match outcome {
                    Err(error) => format!("gh wait failed: {error}"),
                    _ => format!("gh timed out after {} ms", timeout.as_millis()),
                };
                // Kill only the tree this invocation created, including helpers
                // retaining the pipe handles, so draining cannot outlive the deadline.
                let _ = child.kill_tree();
                let _ = child.wait();
                break Err(GhFailure::network(reason));
            }
        }
    };
    let stdout = out
        .join()
        .map_err(|_| GhFailure::network("gh stdout reader failed".to_owned()))?
        .map_err(|error| GhFailure::network(format!("gh stdout: {error}")))?;
    let stderr = err
        .join()
        .map_err(|_| GhFailure::network("gh stderr reader failed".to_owned()))?
        .map_err(|error| GhFailure::network(format!("gh stderr: {error}")))?;
    let status = status?;
    if !status.success() {
        let reason = String::from_utf8_lossy(&stderr).trim().to_owned();
        return Err(classify_failure(
            if reason.is_empty() {
                format!("gh exited with {status}")
            } else {
                reason
            },
            status.code(),
        ));
    }
    String::from_utf8(stdout)
        .map_err(|error| GhFailure::network(format!("gh output was not UTF-8: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture `gh` answers at once; a one-second deadline read a loaded
    /// machine's slow shell start as a timeout, the network category.
    #[cfg(unix)]
    const FIXTURE_DEADLINE: Duration = Duration::from_secs(10);

    #[test]
    fn optional_project_permission_does_not_prevent_basic_issue_resolution() {
        let reference = crate::issues::IssueReference::parse("acme/project#42", None).unwrap();
        let mut attempts = Vec::new();
        let (issues, warning) = with_optional_projects(|include_projects| {
            attempts.push(include_projects);
            let query = issue_query(&[&reference], include_projects)?;
            if include_projects {
                assert!(query.contains("projectItems"));
                return Err(GhFailure::network("INSUFFICIENT_SCOPES: read:project".into()));
            }
            assert!(!query.contains("projectItems"));
            parse_linked_issues(r#"{"data":{"r0":{"issue":{"number":42,"title":"Task","url":"https://github.com/acme/project/issues/42","state":"OPEN","updatedAt":null}}}}"#)
        }).unwrap();
        assert_eq!(attempts, vec![true, false]);
        assert!(warning.is_some());
        assert_eq!(issues[0].reference, reference);
        assert!(issues[0].project_status.is_none());
    }

    #[cfg(unix)]
    struct GhFixture {
        root: PathBuf,
        binary: PathBuf,
    }
    #[cfg(unix)]
    impl GhFixture {
        fn new(body: &str) -> Self {
            static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "hide-gh-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            let binary = root.join("gh");
            crate::executable_fixture::write_executable(&binary, &format!("#!/bin/sh\n{body}\n"));
            Self { root, binary }
        }
    }
    #[cfg(unix)]
    impl Drop for GhFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn an_issue_panel_read_keeps_the_latest_three_comments_and_drops_a_colour_that_is_not_hex() {
        let comment = |n: u32| {
            format!(
                r#"{{"author":{{"login":"c{n}"}},"createdAt":"2026-09-2{n}T00:00:00Z","body":" note {n} "}}"#
            )
        };
        let comments: Vec<String> = (1..=5).map(comment).collect();
        let output = format!(
            r#"{{"body":"본문","labels":[{{"name":"bug","color":"d73a4a"}},{{"name":"odd","color":"red;x"}}],"author":{{"login":"example","name":""}},"assignees":[{{"login":"a1"}}],"comments":[{}],"createdAt":"2026-09-27T00:00:00Z"}}"#,
            comments.join(",")
        );
        let detail = parse_issue_detail(&output).unwrap();
        assert_eq!(detail.body, "본문");
        assert_eq!(
            detail
                .labels
                .iter()
                .map(|label| (label.name.as_str(), label.color.as_deref()))
                .collect::<Vec<_>>(),
            vec![("bug", Some("d73a4a")), ("odd", None)]
        );
        assert_eq!(detail.author.as_deref(), Some("example"));
        assert_eq!(detail.assignees, vec!["a1".to_owned()]);
        assert_eq!(
            detail.created_at_unix_ms,
            parse_rfc3339_ms("2026-09-27T00:00:00Z")
        );
        assert_eq!(detail.comment_count, Some(5));
        assert_eq!(
            detail
                .comments
                .iter()
                .map(|comment| (comment.author.as_deref(), comment.body.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (Some("c3"), "note 3"),
                (Some("c4"), "note 4"),
                (Some("c5"), "note 5")
            ]
        );
        assert!(parse_issue_detail("{}").is_err(), "a body is required");
    }

    #[test]
    #[cfg(unix)]
    fn an_issue_panel_reads_only_its_fields_and_nothing_else_passes_issue_view() {
        let fixture = GhFixture::new(
            r#"
case "$1 $2 $6 $7" in
  "issue view --json body,labels,author,assignees,comments,createdAt") printf '{"body":"b","comments":[]}';;
  *) touch forbidden; exit 91;;
esac"#,
        );
        let viewed = |fields: &str| {
            run_gh(
                &fixture.binary,
                Some(&fixture.root),
                &["issue", "view", "7", "--repo", "acme/app", "--json", fields],
                FIXTURE_DEADLINE,
            )
        };
        assert!(viewed(ISSUE_DETAIL_FIELDS).is_ok());
        assert!(viewed("body,title").is_err());
        assert!(!fixture.root.join("forbidden").exists());
    }

    #[test]
    #[cfg(unix)]
    fn a_closing_line_is_written_once_however_often_it_is_asked_for() {
        // `gh` over a body kept in a file: `pr view` prints it, `pr edit` replaces it.
        let fixture = GhFixture::new(
            r#"
body="$(dirname "$0")/body"
case "$1 $2 $4" in
  "pr view --json") python3 -c 'import json,sys; print(json.dumps({"body": open(sys.argv[1]).read()}))' "$body";;
  "pr edit --body") printf '%s' "$5" > "$body"; echo "https://github.com/acme/app/pull/$3";;
  *) exit 91;;
esac"#,
        );
        let body = fixture.root.join("body");
        std::fs::write(&body, "Moves the reader off the lock.\n").unwrap();
        let gh = |arguments: &[&str]| {
            run_gh(
                &fixture.binary,
                Some(&fixture.root),
                arguments,
                Duration::from_secs(5),
            )
        };
        assert_eq!(write_closing_line_with(gh, 12, 7), Ok(true));
        assert_eq!(
            std::fs::read_to_string(&body).unwrap(),
            "Moves the reader off the lock.\n\nCloses #7"
        );
        // The retry finds the line and writes nothing (D-47, B14).
        assert_eq!(write_closing_line_with(gh, 12, 7), Ok(false));
        assert_eq!(
            std::fs::read_to_string(&body).unwrap(),
            "Moves the reader off the lock.\n\nCloses #7"
        );
    }

    #[test]
    fn a_body_closes_an_issue_only_by_a_closing_keyword_and_its_number() {
        assert!(closes_issue("Fixes #7.", 7));
        assert!(closes_issue("resolves: acme/app#7", 7));
        assert!(closes_issue(
            "closed https://github.com/acme/app/issues/7",
            7
        ));
        assert!(!closes_issue("Closes #70", 7));
        assert!(!closes_issue("See #7", 7));
        assert!(!closes_issue("Closes a/b/c#7", 7));
        assert_eq!(with_closing_line("  \n", 3), "Closes #3");
    }

    #[test]
    fn feedback_names_the_failed_checks_and_the_change_requests_still_standing() {
        let output = r#"{
            "body": "Reads the remote primary.",
            "statusCheckRollup": [
                {"__typename":"CheckRun","name":"verify","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"https://github.com/acme/app/actions/runs/1"},
                {"__typename":"CheckRun","name":"lint","status":"COMPLETED","conclusion":"SUCCESS"},
                {"__typename":"StatusContext","context":"ci/legacy","state":"ERROR","targetUrl":"javascript:alert(1)"}
            ],
            "reviews": [
                {"author":{"login":"ana"},"state":"CHANGES_REQUESTED","body":"old ask"},
                {"author":{"login":"ana"},"state":"COMMENTED","body":"note"},
                {"author":{"login":"ana"},"state":"CHANGES_REQUESTED","body":" Split the reader. "},
                {"author":{"login":"bo"},"state":"CHANGES_REQUESTED","body":"rename"},
                {"author":{"login":"bo"},"state":"APPROVED","body":""}
            ]
        }"#;
        let feedback = parse_pr_feedback(output).unwrap();
        assert_eq!(feedback.body, "Reads the remote primary.");
        assert_eq!(
            feedback.failed_checks,
            vec![
                crate::model::PrFailedCheck {
                    name: "verify".into(),
                    url: Some("https://github.com/acme/app/actions/runs/1".into()),
                },
                crate::model::PrFailedCheck {
                    name: "ci/legacy".into(),
                    url: None,
                },
            ]
        );
        assert_eq!(
            feedback.change_requests,
            vec![crate::model::PrChangeRequest {
                author: Some("ana".into()),
                body: "Split the reader.".into(),
            }]
        );
        assert!(parse_pr_feedback("{}").is_err());
    }

    #[test]
    #[cfg(unix)]
    fn a_pull_request_takes_only_its_body_write_and_its_two_reads() {
        let fixture = GhFixture::new(r#"printf 'ok'"#);
        let run = |arguments: &[&str]| {
            run_gh(
                &fixture.binary,
                Some(&fixture.root),
                arguments,
                Duration::from_secs(5),
            )
        };
        for allowed in [
            &["pr", "view", "12", "--json", "body"][..],
            &["pr", "view", "12", "--json", PR_FEEDBACK_FIELDS],
            &["pr", "edit", "12", "--body", "Closes #7"],
        ] {
            if let Err(failure) = run(allowed) {
                panic!("{allowed:?} must run: {}", failure.reason);
            }
        }
        for refused in [
            &["pr", "edit", "12", "--title", "x"][..],
            &["pr", "edit", "12", "--add-label", "x"],
            &["pr", "edit", "x12", "--body", "y"],
            &["pr", "edit", "12", "--body", "y", "--title", "z"],
            &["pr", "view", "12", "--json", "body,title"],
            &["pr", "merge", "12"],
            &["pr", "comment", "12", "--body", "y"],
            &["pr", "review", "12", "--approve"],
            &["pr", "close", "12"],
        ] {
            assert!(run(refused).is_err(), "{refused:?} must be refused");
        }
    }

    #[test]
    fn ci_rollup_never_reports_absent_pending_or_failed_checks_as_passing() {
        let decode = |json: &str| serde_json::from_str::<Vec<GhCheck>>(json).unwrap();
        assert_eq!(rollup_checks(None), PullRequestChecks::Unknown);
        assert_eq!(rollup_checks(Some(&[])), PullRequestChecks::None);
        assert_eq!(
            rollup_checks(Some(&decode(
                r#"[{"__typename":"CheckRun","status":"COMPLETED","conclusion":"SUCCESS"},{"__typename":"StatusContext","state":"SUCCESS"}]"#
            ))),
            PullRequestChecks::Passing
        );
        assert_eq!(
            rollup_checks(Some(&decode(
                r#"[{"__typename":"CheckRun","status":"QUEUED","conclusion":null}]"#
            ))),
            PullRequestChecks::Pending
        );
        assert_eq!(
            rollup_checks(Some(&decode(
                r#"[{"__typename":"CheckRun","status":"IN_PROGRESS"},{"__typename":"StatusContext","state":"ERROR"}]"#
            ))),
            PullRequestChecks::Failed
        );
        assert_eq!(
            rollup_checks(Some(&decode(
                r#"[{"__typename":"CheckRun","status":"COMPLETED","conclusion":"CANCELLED"}]"#
            ))),
            PullRequestChecks::Failed
        );
        assert_eq!(
            rollup_checks(Some(&decode(r#"[{"__typename":"FutureCheck"}]"#))),
            PullRequestChecks::Unknown
        );
    }

    #[test]
    #[cfg(unix)]
    fn gh_boundary_is_read_only_noninteractive_and_preserves_failure_categories() {
        let fixture = GhFixture::new(
            r#"
[ "$GH_PROMPT_DISABLED" = 1 ] && [ "$GIT_TERMINAL_PROMPT" = 0 ] || exit 90
case "$1 $2" in
  "pr list"|"issue list") printf '[]';;
  "auth status") printf 'not logged into any GitHub hosts' >&2; exit 1;;
  *) touch forbidden; exit 91;;
esac"#,
        );
        assert_eq!(
            run_gh(
                &fixture.binary,
                Some(&fixture.root),
                &["pr", "list"],
                FIXTURE_DEADLINE
            )
            .unwrap(),
            "[]"
        );
        assert_eq!(
            run_gh(
                &fixture.binary,
                Some(&fixture.root),
                &["issue", "list"],
                FIXTURE_DEADLINE
            )
            .unwrap(),
            "[]"
        );
        for write in [
            ["issue", "create"],
            ["issue", "edit"],
            ["issue", "comment"],
            ["issue", "close"],
        ] {
            assert!(
                run_gh(
                    &fixture.binary,
                    Some(&fixture.root),
                    &write,
                    FIXTURE_DEADLINE
                )
                .is_err()
            );
        }
        let failure = run_gh(
            &fixture.binary,
            Some(&fixture.root),
            &["auth", "status"],
            FIXTURE_DEADLINE,
        )
        .unwrap_err();
        assert_eq!(failure.category, "not logged in");
        assert_eq!(failure.reason, "not logged into any GitHub hosts");
        assert!(
            run_gh(
                &fixture.binary,
                Some(&fixture.root),
                &["auth", "login"],
                FIXTURE_DEADLINE
            )
            .is_err()
        );
        assert!(!fixture.root.join("forbidden").exists());
        assert_eq!(
            std::fs::read_dir(&fixture.root).unwrap().count(),
            1,
            "no token or configuration was written"
        );
        assert_eq!(
            run_gh(
                &fixture.root.join("missing"),
                None,
                &["pr", "list"],
                FIXTURE_DEADLINE
            )
            .unwrap_err()
            .category,
            "not installed"
        );
        for (stderr, expected) in [
            (
                "none of the git remotes configured for this repository point to a known GitHub host",
                "no GitHub remote",
            ),
            ("HTTP 429 rate limit exceeded", "network or rate limit"),
        ] {
            let fixture = GhFixture::new(&format!("printf '%s' '{stderr}' >&2; exit 1"));
            let failure =
                run_gh(&fixture.binary, None, &["pr", "list"], FIXTURE_DEADLINE).unwrap_err();
            assert_eq!(failure.category, expected);
            assert_eq!(failure.reason, stderr);
        }
    }

    #[test]
    #[cfg(unix)]
    fn timed_out_gh_and_its_pipe_holding_helper_are_terminated() {
        let fixture = GhFixture::new("echo $$ > pid; sleep 5; touch survived");
        let started = Instant::now();
        // The shell must get a turn to write its PID under parallel workspace
        // tests; the deadline still precedes the script's five-second work.
        let failure = run_gh(
            &fixture.binary,
            Some(&fixture.root),
            &["pr", "list"],
            Duration::from_secs(3),
        )
        .unwrap_err();
        assert_eq!(failure.category, "network or rate limit");
        assert!(failure.reason.contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(5));
        let pid: u32 = std::fs::read_to_string(fixture.root.join("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            !hide_platform::process::is_alive(pid),
            "the gh child has been reaped"
        );
        assert!(!fixture.root.join("survived").exists());
    }

    #[test]
    fn a_project_is_read_first_then_only_when_its_generation_moves() {
        assert_eq!(read_reason(None, 0), Some(ReadReason::First));
        assert_eq!(read_reason(Some(0), 1), Some(ReadReason::Generation));
        assert_eq!(read_reason(Some(1), 1), None);
        assert_eq!(read_reason(Some(1), 1), None);
    }

    fn listed(
        number: u32,
        branch: &str,
        state: &str,
        review: Option<&str>,
        draft: bool,
        updated: &str,
    ) -> String {
        format!(
            r#"{{"title":"Fixture PR","statusCheckRollup":[],"number":{number},"headRefName":"{branch}","baseRefName":"main","state":"{state}","reviewDecision":{review},"isDraft":{draft},"url":"https://example.invalid/{number}","mergedAt":null,"updatedAt":"{updated}"}}"#,
            review = review
                .map(|value| format!("\"{value}\""))
                .unwrap_or_else(|| "null".to_owned())
        )
    }

    #[test]
    fn every_state_and_review_combination_maps_to_its_badge() {
        assert_eq!(badge("MERGED", false, None), PullRequestBadge::Merged);
        assert_eq!(badge("CLOSED", false, None), PullRequestBadge::Closed);
        assert_eq!(badge("OPEN", false, None), PullRequestBadge::Open);
        for decision in [
            ReviewDecision::ReviewRequired,
            ReviewDecision::ChangesRequested,
            ReviewDecision::Approved,
        ] {
            assert_eq!(
                badge("OPEN", false, Some(decision)),
                PullRequestBadge::Review,
                "an open reviewed pull request is a review badge"
            );
            // A draft is not asking for review, whatever GitHub recorded.
            assert_eq!(badge("OPEN", true, Some(decision)), PullRequestBadge::Open);
        }
        // A merged pull request stays merged even if a review is recorded.
        assert_eq!(
            badge("MERGED", false, Some(ReviewDecision::Approved)),
            PullRequestBadge::Merged
        );
    }

    #[test]
    fn the_three_review_decisions_are_kept_apart_and_anything_else_is_none() {
        assert_eq!(
            review_decision(Some("REVIEW_REQUIRED")),
            Some(ReviewDecision::ReviewRequired)
        );
        assert_eq!(
            review_decision(Some("CHANGES_REQUESTED")),
            Some(ReviewDecision::ChangesRequested)
        );
        assert_eq!(
            review_decision(Some("APPROVED")),
            Some(ReviewDecision::Approved)
        );
        assert_eq!(review_decision(Some("")), None);
        assert_eq!(review_decision(None), None);
    }

    fn candidate(
        number: u32,
        branch: &str,
        badge: PullRequestBadge,
        head: Option<&str>,
        updated: u64,
    ) -> PullRequestSnapshot {
        let mut pull_request = parse_pull_requests(&format!(
            "[{}]",
            listed(number, branch, "OPEN", None, false, "2026-09-01T10:00:00Z")
        ))
        .expect("gh output parses")
        .remove(0);
        pull_request.badge = badge;
        pull_request.head_oid = head.map(str::to_owned);
        pull_request.updated_at_unix_ms = Some(updated);
        pull_request
    }

    fn picked(
        pull_requests: &[PullRequestSnapshot],
        branch: &str,
        head: Option<&str>,
    ) -> Option<u32> {
        pull_request_for_checkout(pull_requests, Some(branch), head)
            .map(|pull_request| pull_request.number)
    }

    #[test]
    fn a_listed_pull_request_keeps_its_head_commit_and_whether_it_came_from_a_fork() {
        let output = r#"[{"title":"t","number":7,"headRefName":"feature","headRefOid":"abc123","isCrossRepository":true,"baseRefName":"main","state":"OPEN","reviewDecision":"","isDraft":false,"url":"https://example.invalid/7","mergedAt":null,"updatedAt":"2026-09-01T10:00:00Z"},{"title":"t","number":8,"headRefName":"other","baseRefName":"main","state":"OPEN","reviewDecision":"","isDraft":false,"url":"https://example.invalid/8","mergedAt":null,"updatedAt":"2026-09-01T10:00:00Z"}]"#;
        let listed = parse_pull_requests(output).expect("gh output parses");
        assert_eq!(listed[0].head_oid.as_deref(), Some("abc123"));
        assert!(listed[0].cross_repository);
        assert_eq!(listed[1].head_oid, None);
        assert!(!listed[1].cross_repository);
    }

    #[test]
    fn every_pull_request_of_a_branch_is_kept_for_the_checkout_to_choose_from() {
        let output = format!(
            "[{},{}]",
            listed(9, "feature", "MERGED", None, false, "2026-09-03T10:00:00Z"),
            listed(4, "feature", "OPEN", None, false, "2026-09-01T10:00:00Z"),
        );
        let listed = parse_pull_requests(&output).expect("gh output parses");
        assert_eq!(
            listed.iter().map(|pr| pr.number).collect::<Vec<_>>(),
            vec![9, 4]
        );
    }

    #[test]
    fn an_open_pull_request_beats_a_merged_one_beats_a_closed_one_for_the_same_checkout() {
        let pull_requests = [
            candidate(1, "feature", PullRequestBadge::Closed, Some("head"), 90),
            candidate(2, "feature", PullRequestBadge::Merged, Some("head"), 80),
            candidate(3, "feature", PullRequestBadge::Open, Some("head"), 10),
        ];
        assert_eq!(picked(&pull_requests, "feature", Some("head")), Some(3));
        assert_eq!(
            picked(&pull_requests[..2], "feature", Some("head")),
            Some(2)
        );
        assert_eq!(
            picked(&pull_requests[..1], "feature", Some("head")),
            Some(1)
        );
    }

    #[test]
    fn among_equals_the_most_recently_updated_wins() {
        let pull_requests = [
            candidate(1, "feature", PullRequestBadge::Merged, Some("head"), 10),
            candidate(2, "feature", PullRequestBadge::Merged, Some("head"), 20),
            candidate(3, "feature", PullRequestBadge::Merged, Some("head"), 20),
        ];
        assert_eq!(picked(&pull_requests, "feature", Some("head")), Some(3));
        assert_eq!(
            picked(&pull_requests[..2], "feature", Some("head")),
            Some(2)
        );
    }

    #[test]
    fn an_old_merged_pull_request_does_not_follow_a_branch_name_used_again() {
        let pull_requests = [candidate(
            1,
            "feature",
            PullRequestBadge::Merged,
            Some("old-work"),
            10,
        )];
        assert_eq!(picked(&pull_requests, "feature", Some("new-work")), None);
        assert_eq!(picked(&pull_requests, "feature", Some("old-work")), Some(1));
    }

    #[test]
    fn a_settled_pull_request_needs_a_known_commit_on_both_sides() {
        let closed = candidate(1, "feature", PullRequestBadge::Closed, Some("head"), 10);
        let unread = candidate(2, "feature", PullRequestBadge::Merged, None, 10);
        assert_eq!(picked(std::slice::from_ref(&closed), "feature", None), None);
        assert_eq!(
            picked(std::slice::from_ref(&unread), "feature", Some("head")),
            None
        );
        assert_eq!(
            picked(std::slice::from_ref(&closed), "feature", Some("head")),
            Some(1)
        );
    }

    #[test]
    fn a_settled_pull_request_names_its_branch_as_well_as_its_commit() {
        // A fast-forward merge leaves main on the pull request's head commit;
        // a new branch cut from main there is not that pull request's work.
        let merged = [candidate(
            1,
            "old-feature",
            PullRequestBadge::Merged,
            Some("head"),
            10,
        )];
        assert_eq!(picked(&merged, "new-feature", Some("head")), None);
        assert_eq!(pull_request_for_checkout(&merged, None, Some("head")), None);
    }

    #[test]
    fn an_open_pull_request_belongs_by_branch_whatever_the_checkout_commit_is() {
        let pull_requests = [candidate(
            1,
            "feature",
            PullRequestBadge::Open,
            Some("pushed"),
            10,
        )];
        assert_eq!(
            picked(&pull_requests, "feature", Some("unpushed-commit")),
            Some(1)
        );
        assert_eq!(picked(&pull_requests, "feature", None), Some(1));
        assert_eq!(picked(&pull_requests, "other", Some("pushed")), None);
    }

    #[test]
    fn an_open_pull_request_from_a_fork_never_belongs_to_a_branch_of_the_same_name() {
        let mut from_fork = candidate(1, "main", PullRequestBadge::Open, Some("head"), 10);
        from_fork.cross_repository = true;
        assert_eq!(
            picked(std::slice::from_ref(&from_fork), "main", Some("head")),
            None
        );
        from_fork.cross_repository = false;
        assert_eq!(
            picked(std::slice::from_ref(&from_fork), "main", Some("head")),
            Some(1)
        );
    }

    #[test]
    fn each_branch_lists_its_preferred_pull_request_once() {
        let pull_requests = [
            candidate(1, "alpha", PullRequestBadge::Merged, Some("a"), 10),
            candidate(2, "alpha", PullRequestBadge::Open, Some("b"), 5),
            candidate(3, "beta", PullRequestBadge::Merged, Some("c"), 20),
        ];
        let numbers: Vec<u32> = preferred_per_branch(&pull_requests)
            .into_iter()
            .map(|pull_request| pull_request.number)
            .collect();
        assert_eq!(numbers, vec![2, 3]);
    }

    /// A response Hide cannot read is a stated failure, never an empty list
    /// that would present every branch as having no pull request.
    #[test]
    fn unreadable_output_is_a_reason_rather_than_no_pull_requests() {
        let failure = parse_pull_requests("{\"unexpected\":true}").expect_err("malformed");
        assert!(failure.contains("could not read"), "{failure}");
    }

    #[test]
    fn an_empty_list_is_a_real_answer() {
        assert_eq!(parse_pull_requests("[]").expect("empty list"), Vec::new());
    }

    /// A `gh` that answers the two lists from fixtures and records what it
    /// was asked, so the read can be judged by what it returns and asks for.
    struct TwoLists {
        every: Result<String, GhFailure>,
        open: Result<String, GhFailure>,
        asked: Mutex<Vec<Vec<String>>>,
    }

    impl TwoLists {
        fn new(every: &str, open: &str) -> Self {
            Self {
                every: Ok(every.to_owned()),
                open: Ok(open.to_owned()),
                asked: Mutex::default(),
            }
        }

        fn run(&self, arguments: &[&str]) -> Result<String, GhFailure> {
            self.asked.lock().unwrap().push(
                arguments
                    .iter()
                    .map(|argument| (*argument).to_owned())
                    .collect(),
            );
            let state = arguments[arguments.iter().position(|a| *a == "--state").unwrap() + 1];
            match state {
                "all" => self.every.clone(),
                "open" => self.open.clone(),
                other => panic!("unexpected state {other}"),
            }
        }

        fn read(&self, known: &KnownChecks) -> Result<Vec<PullRequestSnapshot>, GhFailure> {
            list_pull_requests(&|arguments| self.run(arguments), known)
        }
    }

    fn listed_at(number: u32, branch: &str, state: &str, head: &str) -> String {
        format!(
            r#"{{"title":"t","number":{number},"headRefName":"{branch}","headRefOid":"{head}","isCrossRepository":false,"baseRefName":"main","state":"{state}","reviewDecision":"","isDraft":false,"url":"https://example.invalid/{number}","mergedAt":null,"updatedAt":"2026-09-01T10:00:00Z"}}"#
        )
    }

    fn rollup(number: u32, conclusion: &str) -> String {
        format!(
            r#"{{"number":{number},"statusCheckRollup":[{{"__typename":"CheckRun","status":"COMPLETED","conclusion":"{conclusion}"}}]}}"#
        )
    }

    fn checks_of(pull_requests: &[PullRequestSnapshot], number: u32) -> PullRequestChecks {
        pull_requests
            .iter()
            .find(|pull_request| pull_request.number == number)
            .expect("pull request listed")
            .checks
    }

    #[test]
    fn open_pull_requests_take_their_checks_from_the_open_list_and_settled_ones_read_unknown() {
        let gh = TwoLists::new(
            &format!(
                "[{},{},{}]",
                listed_at(1, "a", "OPEN", "h1"),
                listed_at(2, "b", "MERGED", "h2"),
                listed_at(3, "c", "CLOSED", "h3"),
            ),
            &format!("[{}]", rollup(1, "FAILURE")),
        );
        let read = gh.read(&KnownChecks::new()).expect("both lists answer");
        assert_eq!(checks_of(&read, 1), PullRequestChecks::Failed);
        assert_eq!(checks_of(&read, 2), PullRequestChecks::Unknown);
        assert_eq!(checks_of(&read, 3), PullRequestChecks::Unknown);
    }

    #[test]
    fn a_settled_pull_request_keeps_the_checks_read_for_its_number_and_head_commit() {
        let gh = TwoLists::new(
            &format!(
                "[{},{}]",
                listed_at(2, "b", "MERGED", "h2"),
                listed_at(3, "c", "MERGED", "moved-on"),
            ),
            "[]",
        );
        let known: KnownChecks = [
            ((2, "h2".to_owned()), PullRequestChecks::Passing),
            ((3, "h3".to_owned()), PullRequestChecks::Failed),
        ]
        .into_iter()
        .collect();
        let read = gh.read(&known).expect("both lists answer");
        assert_eq!(checks_of(&read, 2), PullRequestChecks::Passing);
        assert_eq!(
            checks_of(&read, 3),
            PullRequestChecks::Unknown,
            "checks read at another head commit are not this commit's"
        );
    }

    #[test]
    fn what_was_read_is_kept_for_the_next_read_and_survives_a_failed_one() {
        let gh = TwoLists::new(
            &format!("[{}]", listed_at(1, "a", "OPEN", "h1")),
            &format!("[{}]", rollup(1, "SUCCESS")),
        );
        let read = gh.read(&KnownChecks::new()).expect("both lists answer");
        let answer = GithubProjectSnapshot {
            pull_requests: read,
            pull_requests_read: true,
            ..GithubProjectSnapshot::default()
        };
        let kept = carried_checks(&answer, KnownChecks::new());
        assert_eq!(
            kept.get(&(1, "h1".to_owned())),
            Some(&PullRequestChecks::Passing)
        );
        let failed = GithubProjectSnapshot::default();
        assert_eq!(carried_checks(&failed, kept.clone()), kept);
    }

    #[test]
    fn the_two_lists_ask_for_what_each_is_for() {
        let gh = TwoLists::new("[]", "[]");
        gh.read(&KnownChecks::new()).expect("both lists answer");
        let asked = gh.asked.lock().unwrap();
        let fields = |state: &str| -> String {
            let call = asked
                .iter()
                .find(|call| call.iter().any(|argument| argument == state))
                .expect("the list was asked for");
            call[call.iter().position(|a| a == "--json").unwrap() + 1].clone()
        };
        assert_eq!(asked.len(), 2);
        assert!(
            !fields("all").contains("statusCheckRollup"),
            "settled pull requests' checks are not asked for"
        );
        assert!(
            fields("all").contains("headRefOid") && fields("all").contains("isCrossRepository")
        );
        assert_eq!(fields("open"), "number,statusCheckRollup");
    }

    #[test]
    fn either_list_failing_fails_the_read_instead_of_reading_as_no_pull_requests() {
        let mut gh = TwoLists::new("[]", "[]");
        gh.open = Err(GhFailure::network("open list down".to_owned()));
        let failure = gh.read(&KnownChecks::new()).expect_err("open list failed");
        assert_eq!(failure.reason, "open list down");

        let mut gh = TwoLists::new("[]", "[]");
        gh.every = Err(GhFailure::network("all list down".to_owned()));
        let failure = gh.read(&KnownChecks::new()).expect_err("all list failed");
        assert_eq!(failure.reason, "all list down");

        let gh = TwoLists::new("[]", "{\"unexpected\":true}");
        let failure = gh.read(&KnownChecks::new()).expect_err("unreadable checks");
        assert!(
            failure.reason.contains("could not read"),
            "{}",
            failure.reason
        );
    }

    /// The two reads have to overlap or the saving is gone: each waits for
    /// the other to have started, and a read that runs them one after the
    /// other gives up after the deadline instead of hanging.
    #[test]
    fn the_two_lists_are_asked_for_at_the_same_time() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let started = AtomicUsize::new(0);
        let run = |_: &[&str]| -> Result<String, GhFailure> {
            started.fetch_add(1, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_secs(10);
            while started.load(Ordering::SeqCst) < 2 {
                if Instant::now() > deadline {
                    return Err(GhFailure::network(
                        "the other list was never asked".to_owned(),
                    ));
                }
                std::thread::yield_now();
            }
            Ok("[]".to_owned())
        };
        list_pull_requests(&run, &KnownChecks::new()).expect("both lists were in flight together");
    }

    #[test]
    fn merged_pull_request_proof_keeps_exact_head_and_base() {
        let proofs = parse_merged_pull_request_proofs(
            r#"[{"headRefOid":"branch-head","baseRefName":"main"},{"headRefOid":"other-head","baseRefName":"ci/platform-completion"}]"#,
        )
        .expect("merge proof parses");
        assert_eq!(
            proofs,
            vec![
                MergedPullRequestProof {
                    head_oid: "branch-head".into(),
                    base_ref: "main".into(),
                },
                MergedPullRequestProof {
                    head_oid: "other-head".into(),
                    base_ref: "ci/platform-completion".into(),
                },
            ]
        );
    }

    #[test]
    fn timestamps_convert_to_the_instant_the_card_counts_from() {
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_ms("2026-09-03T04:15:00Z"),
            Some(1_788_408_900_000)
        );
        assert_eq!(parse_rfc3339_ms("not a timestamp"), None);
    }

    #[test]
    fn issue_dependencies_are_read_in_one_query_and_only_open_blockers_block() {
        let issue = |repository: &str, number| crate::issues::IssueSnapshot {
            reference: crate::issues::IssueReference {
                repository: repository.into(),
                number,
            },
            title: String::new(),
            url: String::new(),
            state: "OPEN".into(),
            project_status: None,
            updated_at_unix_ms: None,
            created_at_unix_ms: None,
            closed_at_unix_ms: None,
            blocked_by: Vec::new(),
        };
        let query = dependency_query(&[
            issue("acme/app", 171),
            issue("acme/app", 172),
            issue("acme/other", 9),
        ])
        .unwrap();
        assert!(query.starts_with("query HideIssueDependencies {"));
        assert_eq!(query.matches(":repository(").count(), 2);
        assert!(query.contains("i171:issue(number:171)") && query.contains("i9:issue(number:9)"));
        assert!(query.contains("blockedBy(first:20)"));
        assert!(
            run_gh(
                Path::new("/nonexistent/gh"),
                None,
                &["api", "graphql", "-f", &format!("query={query}")],
                COMMAND_TIMEOUT
            )
            .is_err_and(|failure| failure.category == "not installed")
        );

        let answer = serde_json::json!({"data": {
            "r0": {"nameWithOwner": "acme/app",
                "i171": {"number": 171, "blockedBy": {"nodes": [
                    {"number": 170, "state": "OPEN", "repository": {"nameWithOwner": "acme/app"}},
                    {"number": 160, "state": "CLOSED", "repository": {"nameWithOwner": "acme/app"}},
                ]}},
                "i172": {"number": 172, "blockedBy": {"nodes": [
                    {"number": 3, "state": "OPEN", "repository": {"nameWithOwner": "acme/other"}},
                ]}}},
            "r1": {"nameWithOwner": "acme/other", "i9": null},
        }});
        let blockers = parse_dependencies(&answer.to_string()).unwrap();
        let reference = |value: &str| crate::issues::IssueReference::parse(value, None).unwrap();
        assert_eq!(
            blockers[&reference("acme/app#171")],
            vec![reference("acme/app#170")]
        );
        assert_eq!(
            blockers[&reference("acme/app#172")],
            vec![reference("acme/other#3")]
        );
        assert_eq!(
            blockers.len(),
            2,
            "an issue GitHub could not return has no entry"
        );
    }

    #[test]
    fn an_issue_dependency_error_is_a_failure_not_an_empty_answer() {
        let errors =
            serde_json::json!({"errors": [{"message": "Field 'blockedBy' doesn't exist"}]});
        assert!(parse_dependencies(&errors.to_string()).is_err());
        assert!(parse_dependencies("not json").is_err());
    }

    /// What `gh search prs` and `gh search issues` printed against cli/cli
    /// with `--json isDraft,number,repository,state,title,url` (gh 2.76), plus
    /// a hit from another repository, which a `repo:` qualifier in the query
    /// brings in.
    const SEARCH_PRS_OUTPUT: &str = r#"[{"isDraft":true,"number":10253,"repository":{"name":"cli","nameWithOwner":"cli/cli"},"state":"open","title":"Include headRepositoryId when creating a new PR","url":"https://github.com/cli/cli/pull/10253"},{"isDraft":false,"number":14054,"repository":{"name":"cli","nameWithOwner":"cli/cli"},"state":"closed","title":"fix: report actual error when auth status token check fails for non-401","url":"https://github.com/cli/cli/pull/14054"},{"isDraft":false,"number":13949,"repository":{"name":"cli","nameWithOwner":"cli/cli"},"state":"merged","title":"merged one","url":"https://github.com/cli/cli/pull/13949"},{"isDraft":false,"number":309,"repository":{"name":"go-gh","nameWithOwner":"cli/go-gh"},"state":"open","title":"elsewhere","url":"https://github.com/cli/go-gh/pull/309"}]"#;
    const SEARCH_ISSUES_OUTPUT: &str = r#"[{"number":14046,"repository":{"name":"cli","nameWithOwner":"cli/cli"},"state":"open","title":"Unable to use `gh pr view --web` when sandbox (Docker sbx) remote exists","url":"https://github.com/cli/cli/issues/14046"}]"#;

    #[test]
    fn search_hits_read_as_gh_prints_them_and_stay_inside_the_repository_asked() {
        let prs = parse_search(SEARCH_PRS_OUTPUT, SearchKind::Pr, &["cli/cli".to_owned()]).unwrap();
        assert_eq!(
            prs.iter()
                .map(|hit| (
                    hit.kind.as_str(),
                    hit.number,
                    hit.state.as_str(),
                    hit.is_draft
                ))
                .collect::<Vec<_>>(),
            vec![
                ("pr", 10253, "open", true),
                ("pr", 14054, "closed", false),
                ("pr", 13949, "merged", false),
            ],
            "the go-gh hit is another repository's"
        );
        assert_eq!(prs[0].repository, "cli/cli");
        assert_eq!(prs[0].url, "https://github.com/cli/cli/pull/10253");
        let issues = parse_search(
            SEARCH_ISSUES_OUTPUT,
            SearchKind::Issue,
            &["CLI/cli".to_owned()],
        )
        .unwrap();
        assert_eq!(issues.len(), 1, "GitHub names ignore case");
        assert_eq!(
            (
                issues[0].kind.as_str(),
                issues[0].is_draft,
                issues[0].number
            ),
            ("issue", false, 14046)
        );
        assert_eq!(
            parse_search("[]", SearchKind::Pr, &["cli/cli".to_owned()]).unwrap(),
            vec![]
        );
    }

    #[test]
    fn a_search_answer_hide_cannot_read_is_a_failure_not_an_empty_result() {
        for output in [
            "",
            "{}",
            "not json",
            r#"[{"number":1,"repository":{"nameWithOwner":"a/b"},"state":"open","title":"t"}]"#,
            r#"[{"number":1,"repository":{"nameWithOwner":"a/b"},"state":"draft","title":"t","url":"https://github.com/a/b/pull/1"}]"#,
            r#"[{"number":1,"repository":{"nameWithOwner":"a/b"},"state":"open","title":"t","url":"javascript:alert(1)"}]"#,
        ] {
            assert!(
                parse_search(output, SearchKind::Pr, &["a/b".to_owned()]).is_err(),
                "{output:?}"
            );
        }
    }

    fn hit_json(repository: &str, kind: &str, number: u32) -> String {
        let path = if kind == "pr" { "pull" } else { "issues" };
        format!(
            r#"{{"isDraft":false,"number":{number},"repository":{{"nameWithOwner":"{repository}"}},"state":"open","title":"t{number}","url":"https://github.com/{repository}/{path}/{number}"}}"#
        )
    }

    #[test]
    fn a_search_makes_one_call_per_kind_naming_every_repository_with_the_query_as_words() {
        let calls = std::cell::RefCell::new(Vec::new());
        let run = |arguments: &[&str]| {
            calls.borrow_mut().push(arguments.join(" "));
            let kind = if arguments[1] == "prs" { "pr" } else { "issue" };
            // Thirty hits a call: more than the cap, across both repositories.
            Ok(format!(
                "[{}]",
                (0..30)
                    .map(|n| hit_json(if n % 2 == 0 { "acme/a" } else { "acme/b" }, kind, 100 + n))
                    .collect::<Vec<_>>()
                    .join(",")
            ))
        };
        let repositories = vec!["acme/a".to_owned(), "acme/b".to_owned()];
        let found = search_with(run, &repositories, "reader  --limit").unwrap();
        assert_eq!(found.len(), 40);
        let kinds: Vec<_> = found.iter().map(|hit| hit.kind.as_str()).collect();
        assert_eq!(kinds[..20], ["pr"; 20][..], "pull requests first");
        assert_eq!(kinds[20..], ["issue"; 20][..]);
        let calls = calls.into_inner();
        assert_eq!(
            calls,
            vec![
                "search prs --repo acme/a --repo acme/b --limit 20 --json isDraft,number,repository,state,title,url -- reader --limit",
                "search issues --repo acme/a --repo acme/b --limit 20 --json number,repository,state,title,url -- reader --limit",
            ],
            "two calls however many repositories, the query as separate words"
        );
        assert_eq!(search_with(|_| Ok("[]".into()), &[], "q").unwrap(), vec![]);
    }

    #[test]
    fn one_failed_search_call_fails_the_whole_search_and_names_the_call() {
        let run = |arguments: &[&str]| {
            if arguments[1] == "issues" {
                Err(GhFailure::network(
                    "HTTP 403: API rate limit exceeded".into(),
                ))
            } else {
                Ok("[]".to_owned())
            }
        };
        let repositories = vec!["acme/a".to_owned(), "acme/b".to_owned()];
        let error = search_with(run, &repositories, "q").unwrap_err();
        assert!(error.contains("gh search issues"), "{error}");
        assert!(error.contains("rate limit"), "{error}");
    }

    #[test]
    fn an_unread_project_is_resolved_by_the_search_and_an_unreadable_one_fails_it() {
        let target = |root: &str, repository: Option<&str>| SearchTarget {
            root: root.into(),
            repository: repository.map(str::to_owned),
        };
        let run = |cwd: Option<&Path>, arguments: &[&str]| {
            assert_eq!(arguments, ["repo", "view", "--json", "nameWithOwner"]);
            match cwd.and_then(Path::to_str) {
                Some("/unread") => Ok(r#"{"nameWithOwner":"ACME/Known"}"#.to_owned()),
                Some("/new") => Ok(r#"{"nameWithOwner":"acme/new"}"#.to_owned()),
                Some("/gitlab") => Err(GhFailure {
                    category: "no GitHub remote",
                    reason: "none of the git remotes point to a known GitHub host".into(),
                }),
                _ => Err(GhFailure::network("HTTP 502".into())),
            }
        };
        // Resolved ones join the known ones once, ignoring case; a project with
        // no GitHub remote is not one to search.
        let found = resolve_repositories(
            run,
            &[
                target("/known", Some("acme/known")),
                target("/unread", None),
                target("/new", None),
                target("/gitlab", None),
            ],
        )
        .unwrap();
        assert_eq!(found, vec!["acme/known", "acme/new"]);
        assert!(
            resolve_repositories(run, &[target("/gitlab", None)])
                .unwrap()
                .is_empty()
        );
        // Any other failure fails the search: it cannot say it covered the project.
        let error = resolve_repositories(run, &[target("/new", None), target("/broken", None)])
            .unwrap_err();
        assert!(
            error.contains("/broken") && error.contains("502"),
            "{error}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_search_takes_its_repositories_the_cap_fixed_fields_and_the_query_words_after_the_dashes() {
        let fixture = GhFixture::new(r#"printf '[]'"#);
        let allowed = |arguments: &[&str]| {
            run_gh(&fixture.binary, None, arguments, Duration::from_secs(5)).is_ok()
        };
        let prs = |repositories: &[&'static str], words: &[&'static str]| {
            let mut arguments = vec!["search", "prs"];
            for repository in repositories {
                arguments.extend(["--repo", repository]);
            }
            arguments.extend(["--limit", "20", "--json", SEARCH_PR_FIELDS, "--"]);
            arguments.extend(words);
            arguments
        };
        assert!(allowed(&prs(&["acme/app"], &["reader"])));
        assert!(allowed(&prs(
            &["acme/app", "acme/other"],
            &["two", "words"]
        )));
        assert!(
            allowed(&prs(&["acme/app"], &["--web"])),
            "a word is never a flag"
        );
        assert!(allowed(&[
            "search",
            "issues",
            "--repo",
            "acme/app",
            "--limit",
            "20",
            "--json",
            SEARCH_ISSUE_FIELDS,
            "--",
            "-w",
        ]));
        let twenty = ["acme/app"; SEARCH_REPOSITORY_LIMIT];
        assert!(allowed(&prs(&twenty, &["q"])));
        let long = "x".repeat(SEARCH_QUERY_LIMIT + 1);
        let long: &'static str = Box::leak(long.into_boxed_str());
        let twenty_one = ["acme/app"; SEARCH_REPOSITORY_LIMIT + 1];
        let refused: Vec<Vec<&str>> = vec![
            prs(&["acme/app"], &[""]),
            prs(&["acme/app"], &["  "]),
            prs(&["acme/app"], &[]),
            prs(&["acme/app"], &[long]),
            prs(&["acme/app"], &[&long[..150], &long[..100]]),
            prs(&twenty_one, &["q"]),
            prs(&[], &["q"]),
            prs(&["acme/app/extra"], &["q"]),
            prs(&["acme"], &["q"]),
            prs(&["--repo"], &["q"]),
            prs(&["acme/../app"], &["q"]),
            // A different limit, field list, flag position or subcommand.
            vec![
                "search",
                "prs",
                "--repo",
                "acme/app",
                "--limit",
                "21",
                "--json",
                SEARCH_PR_FIELDS,
                "--",
                "q",
            ],
            vec![
                "search",
                "prs",
                "--repo",
                "acme/app",
                "--limit",
                "20",
                "--json",
                SEARCH_ISSUE_FIELDS,
                "--",
                "q",
            ],
            vec![
                "search",
                "issues",
                "--repo",
                "acme/app",
                "--limit",
                "20",
                "--json",
                SEARCH_PR_FIELDS,
                "--",
                "q",
            ],
            vec![
                "search", "prs", "--repo", "acme/app", "--limit", "20", "--json", "body", "--", "q",
            ],
            vec![
                "search",
                "prs",
                "--limit",
                "20",
                "--repo",
                "acme/app",
                "--json",
                SEARCH_PR_FIELDS,
                "--",
                "q",
            ],
            vec![
                "search",
                "prs",
                "--repo",
                "acme/app",
                "--limit",
                "20",
                "--json",
                SEARCH_PR_FIELDS,
                "q",
                "--",
            ],
            vec![
                "search",
                "prs",
                "--repo",
                "acme/app",
                "--limit",
                "20",
                "--json",
                SEARCH_PR_FIELDS,
                "q",
            ],
            vec![
                "search",
                "code",
                "--repo",
                "acme/app",
                "--limit",
                "20",
                "--json",
                SEARCH_PR_FIELDS,
                "--",
                "q",
            ],
            vec![
                "search", "repos", "--limit", "20", "--json", "name", "--", "q",
            ],
            vec!["search", "prs", "q"],
            vec!["search", "prs", "--web", "q"],
        ];
        for arguments in &refused {
            assert!(!allowed(arguments), "{arguments:?} must be refused");
        }
    }
}
