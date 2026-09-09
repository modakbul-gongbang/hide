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
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::model::{
    GithubProjectSnapshot, GithubSnapshot, GithubStatusSnapshot, PullRequestBadge,
    PullRequestChecks, PullRequestSnapshot, ReviewDecision,
};
use crate::reader::BackgroundRead;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);

/// Every pull request `gh` will return in one call. Past this, older pull
/// requests are simply absent and their branches read as having none; the
/// limit is stated here so the risk is findable from the code that takes it.
pub(crate) const PULL_REQUEST_LIMIT: &str = "200";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GithubProjectRequest {
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

struct CachedProject {
    generation: u64,
    answer: GithubProjectSnapshot,
}

/// The last answer per requested root, kept on the worker's side so a pass
/// re-reads only the projects that are due and hands the rest back as they
/// were. It is the reader's own state, never the runtime's: the runtime
/// receives a whole snapshot every time and does not know which half is new.
type Cache = Arc<Mutex<HashMap<PathBuf, CachedProject>>>;

pub struct GithubReader {
    inner: BackgroundRead<GithubRequest, GithubSnapshot>,
}

impl GithubReader {
    pub fn new() -> Self {
        let cache: Cache = Arc::default();
        Self {
            inner: BackgroundRead::on_change(Duration::ZERO, move |request| read(&cache, request)),
        }
    }

    pub fn read_if_due(&mut self, request: GithubRequest) -> Option<GithubSnapshot> {
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
            let answer = read_root(&authentication, &project.root, project.generation);
            cache.insert(
                project.root.clone(),
                CachedProject {
                    generation: project.generation,
                    answer,
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
) -> GithubProjectSnapshot {
    let Some(main) = main_worktree(root) else {
        return GithubProjectSnapshot::default();
    };
    let root_path = main.to_string_lossy().into_owned();
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
        Ok(()) => read_project(&main, root_path),
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
        "message": project.status.unavailable_reason,
    }));
    project
}

/// Authentication is read-only and uses the same bounded subprocess as PR listing.
fn authentication() -> Result<(), GhFailure> {
    gh(None, &["auth", "status"]).map(|_| ())
}

fn read_project(root: &Path, root_path: String) -> GithubProjectSnapshot {
    let listed = match gh(
        Some(root),
        &[
            "pr",
            "list",
            "--state",
            "all",
            "--limit",
            PULL_REQUEST_LIMIT,
            "--json",
            "number,title,statusCheckRollup,headRefName,baseRefName,state,reviewDecision,isDraft,url,mergedAt,updatedAt",
        ],
    ) {
        Ok(listed) => listed,
        Err(reason) => {
            return GithubProjectSnapshot {
                root_path,
                status: failed(reason),
                ..GithubProjectSnapshot::default()
            };
        }
    };

    match parse_pull_requests(&listed) {
        Ok(pull_requests) => GithubProjectSnapshot {
            root_path,
            status: GithubStatusSnapshot {
                available: true,
                loading: false,
                stale: false,
                last_success_at_unix_ms: Some(now_unix_ms()),
                unavailable_reason: None,
                failure_category: None,
            },
            pull_requests,
        },
        // A response shape Hide cannot read is a failure with a reason, never
        // an empty pull-request list: "no pull requests" is a real answer and
        // must not be manufactured out of a parse error.
        Err(reason) => GithubProjectSnapshot {
            root_path,
            status: failed(GhFailure::network(reason)),
            ..GithubProjectSnapshot::default()
        },
    }
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
    status_check_rollup: Option<Vec<GhCheck>>,
    number: u32,
    head_ref_name: String,
    base_ref_name: String,
    state: String,
    review_decision: Option<String>,
    is_draft: bool,
    url: String,
    merged_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "__typename")]
enum GhCheck {
    CheckRun {
        status: String,
        conclusion: Option<String>,
    },
    StatusContext {
        state: String,
    },
    #[serde(other)]
    Unknown,
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
            GhCheck::CheckRun { status, conclusion } if status == "COMPLETED" => {
                match conclusion.as_deref() {
                    Some("SUCCESS" | "NEUTRAL" | "SKIPPED") => {}
                    Some(
                        "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED"
                        | "STARTUP_FAILURE" | "STALE",
                    ) => return PullRequestChecks::Failed,
                    _ => unknown = true,
                }
            }
            GhCheck::CheckRun { status, .. }
                if matches!(
                    status.as_str(),
                    "QUEUED" | "IN_PROGRESS" | "WAITING" | "PENDING" | "REQUESTED"
                ) =>
            {
                pending = true
            }
            GhCheck::StatusContext { state } => match state.as_str() {
                "SUCCESS" => {}
                "FAILURE" | "ERROR" => return PullRequestChecks::Failed,
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

/// Reduces `gh pr list --json` to at most one pull request per branch.
pub fn parse_pull_requests(output: &str) -> Result<Vec<PullRequestSnapshot>, String> {
    let listed: Vec<GhPullRequest> = serde_json::from_str(output)
        .map_err(|error| format!("gh pr list returned output Hide could not read: {error}"))?;
    Ok(select_per_branch(
        listed.into_iter().map(project).collect::<Vec<_>>(),
    ))
}

fn project(listed: GhPullRequest) -> PullRequestSnapshot {
    let review = review_decision(listed.review_decision.as_deref());
    PullRequestSnapshot {
        title: listed.title,
        checks: rollup_checks(listed.status_check_rollup.as_deref()),
        badge: badge(&listed.state, listed.is_draft, review),
        number: listed.number,
        head_branch: listed.head_ref_name,
        base_branch: listed.base_ref_name,
        url: listed.url,
        review: review.filter(|_| !listed.is_draft && listed.state.eq_ignore_ascii_case("OPEN")),
        is_draft: listed.is_draft,
        merged_at_unix_ms: listed.merged_at.as_deref().and_then(parse_rfc3339_ms),
        updated_at_unix_ms: listed.updated_at.as_deref().and_then(parse_rfc3339_ms),
    }
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

/// One pull request per branch: an open one beats a settled one, and among
/// equals the most recently updated wins.
///
/// A branch that was merged and then reopened for more work must show the work
/// in flight, not the merge behind it - that is the whole reason open comes
/// first rather than latest-wins alone.
pub fn select_per_branch(mut candidates: Vec<PullRequestSnapshot>) -> Vec<PullRequestSnapshot> {
    candidates.sort_by(|left, right| {
        left.head_branch
            .cmp(&right.head_branch)
            .then_with(|| is_open(right).cmp(&is_open(left)))
            .then_with(|| right.updated_at_unix_ms.cmp(&left.updated_at_unix_ms))
            .then_with(|| right.number.cmp(&left.number))
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
    let leap = |year: u64| year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
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

fn run_gh(
    binary: &Path,
    cwd: Option<&Path>,
    arguments: &[&str],
    timeout: Duration,
) -> Result<String, GhFailure> {
    if !(arguments.starts_with(&["auth", "status"]) || arguments.starts_with(&["pr", "list"])) {
        return Err(GhFailure::network(
            "Unsupported read-only gh command".to_owned(),
        ));
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
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(|error| GhFailure {
        category: if error.kind() == std::io::ErrorKind::NotFound {
            "not installed"
        } else {
            "network or rate limit"
        },
        reason: format!("gh could not be run: {error}"),
    })?;
    // Drain both pipes while waiting, otherwise a large PR list fills stdout
    // and the child cannot exit before the timeout.
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
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
                // Kill only the group this invocation created, including helpers
                // retaining the pipe handles, so draining cannot outlive the deadline.
                #[cfg(unix)]
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
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

    #[cfg(unix)]
    struct GhFixture {
        root: PathBuf,
        binary: PathBuf,
    }
    #[cfg(unix)]
    impl GhFixture {
        fn new(body: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let root = std::env::temp_dir().join(format!(
                "hide-gh-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&root).unwrap();
            let binary = root.join("gh");
            std::fs::write(&binary, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
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
  "pr list") printf '[]';;
  "auth status") printf 'not logged into any GitHub hosts' >&2; exit 1;;
  *) touch forbidden; exit 91;;
esac"#,
        );
        assert_eq!(
            run_gh(
                &fixture.binary,
                Some(&fixture.root),
                &["pr", "list"],
                Duration::from_secs(1)
            )
            .unwrap(),
            "[]"
        );
        let failure = run_gh(
            &fixture.binary,
            Some(&fixture.root),
            &["auth", "status"],
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert_eq!(failure.category, "not logged in");
        assert_eq!(failure.reason, "not logged into any GitHub hosts");
        assert!(
            run_gh(
                &fixture.binary,
                Some(&fixture.root),
                &["auth", "login"],
                Duration::from_secs(1)
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
                Duration::from_secs(1)
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
            let failure = run_gh(
                &fixture.binary,
                None,
                &["pr", "list"],
                Duration::from_secs(1),
            )
            .unwrap_err();
            assert_eq!(failure.category, expected);
            assert_eq!(failure.reason, stderr);
        }
    }

    #[test]
    #[cfg(unix)]
    fn timed_out_gh_and_its_pipe_holding_helper_are_terminated() {
        let fixture = GhFixture::new("echo $$ > pid; sleep 5; touch survived");
        let started = Instant::now();
        let failure = run_gh(
            &fixture.binary,
            Some(&fixture.root),
            &["pr", "list"],
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert_eq!(failure.category, "network or rate limit");
        assert!(failure.reason.contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(3));
        let pid: i32 = std::fs::read_to_string(fixture.root.join("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
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

    #[test]
    fn an_open_pull_request_beats_a_merged_one_on_the_same_branch() {
        let output = format!(
            "[{},{}]",
            listed(9, "feature", "MERGED", None, false, "2026-09-03T10:00:00Z"),
            listed(4, "feature", "OPEN", None, false, "2026-09-01T10:00:00Z"),
        );
        let selected = parse_pull_requests(&output).expect("gh output parses");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].number, 4);
        assert_eq!(selected[0].badge, PullRequestBadge::Open);
    }

    #[test]
    fn among_equals_the_most_recently_updated_wins() {
        let output = format!(
            "[{},{}]",
            listed(1, "feature", "CLOSED", None, false, "2026-08-01T10:00:00Z"),
            listed(2, "feature", "MERGED", None, false, "2026-09-01T10:00:00Z"),
        );
        let selected = parse_pull_requests(&output).expect("gh output parses");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].number, 2);
        assert_eq!(selected[0].badge, PullRequestBadge::Merged);
    }

    #[test]
    fn each_branch_keeps_its_own_pull_request() {
        let output = format!(
            "[{},{}]",
            listed(
                1,
                "alpha",
                "OPEN",
                Some("APPROVED"),
                false,
                "2026-09-01T10:00:00Z"
            ),
            listed(2, "beta", "MERGED", None, false, "2026-09-02T10:00:00Z"),
        );
        let selected = parse_pull_requests(&output).expect("gh output parses");
        assert_eq!(selected.len(), 2);
        let alpha = selected
            .iter()
            .find(|pr| pr.head_branch == "alpha")
            .expect("alpha");
        assert_eq!(alpha.badge, PullRequestBadge::Review);
        assert_eq!(alpha.review, Some(ReviewDecision::Approved));
        let beta = selected
            .iter()
            .find(|pr| pr.head_branch == "beta")
            .expect("beta");
        assert_eq!(beta.badge, PullRequestBadge::Merged);
        assert_eq!(beta.review, None);
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

    #[test]
    fn timestamps_convert_to_the_instant_the_card_counts_from() {
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_ms("2026-09-03T04:15:00Z"),
            Some(1_788_408_900_000)
        );
        assert_eq!(parse_rfc3339_ms("not a timestamp"), None);
    }
}
