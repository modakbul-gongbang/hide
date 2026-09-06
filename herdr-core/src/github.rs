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
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::model::{
    GithubProjectSnapshot, GithubSnapshot, GithubStatusSnapshot, PullRequestBadge,
    PullRequestSnapshot, ReviewDecision,
};
use crate::reader::BackgroundRead;

/// How often a repository's pull requests are re-read on their own.
///
/// A pull request is opened, reviewed, and merged by other people on their
/// schedule, so there is nothing local to key off. Five minutes is the
/// interview's decision: often enough that a merge shows up while the operator
/// is still working, rare enough that a dozen projects cost a dozen `gh` calls
/// an hour rather than a minute.
const REFRESH_INTERVAL: Duration = Duration::from_secs(300);

/// The least time between two reads of the same project list when a trigger
/// keeps firing. A project with a dozen agents in it sees one leave `working`
/// every few seconds, and each would otherwise be two `gh` subprocesses; at
/// this spacing the worst case is three reads a minute, and a trigger inside
/// the wait is served by the read that starts when the wait ends.
const TRIGGER_SPACING: Duration = Duration::from_secs(20);

/// Every pull request `gh` will return in one call. Past this, older pull
/// requests are simply absent and their branches read as having none; the
/// limit is stated here so the risk is findable from the code that takes it.
const PULL_REQUEST_LIMIT: &str = "200";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GithubProjectRequest {
    /// A path inside the repository.
    pub root: PathBuf,
    /// Bumped by the card's refresh button and by an agent leaving `working`
    /// in one of this project's checkouts. Both are "read this project again
    /// now", and both coalesce into one read because they move the same
    /// counter. Another project's counter does not move it, so an agent
    /// finishing elsewhere costs this project nothing.
    pub generation: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GithubRequest {
    pub projects: Vec<GithubProjectRequest>,
}

/// Why one project is read on this pass, stated in the read log so "did the
/// refresh button / the agent finishing / the five-minute window actually
/// re-read?" is answered afterwards without a debugger (G5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadReason {
    First,
    Generation,
    Interval,
}

impl ReadReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Generation => "generation",
            Self::Interval => "interval",
        }
    }
}

/// Whether a project must be read again, given what the reader last answered
/// for it: nothing yet, an answer for an older request, or an answer that has
/// outlived the window. A fresh answer for the same request is reused, which
/// is what keeps one project's trigger from re-reading every other project.
fn read_reason(cached: Option<(u64, Duration)>, generation: u64) -> Option<ReadReason> {
    match cached {
        None => Some(ReadReason::First),
        Some((read_generation, _)) if read_generation != generation => Some(ReadReason::Generation),
        Some((_, age)) if age >= REFRESH_INTERVAL => Some(ReadReason::Interval),
        Some(_) => None,
    }
}

struct CachedProject {
    generation: u64,
    read_at: Instant,
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
            inner: BackgroundRead::new(REFRESH_INTERVAL, TRIGGER_SPACING, move |request| {
                read(&cache, request)
            }),
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
    let now = Instant::now();
    let due: Vec<(&GithubProjectRequest, ReadReason)> = request
        .projects
        .iter()
        .filter_map(|project| {
            let cached = cache
                .get(&project.root)
                .map(|cached| (cached.generation, now.duration_since(cached.read_at)));
            read_reason(cached, project.generation).map(|reason| (project, reason))
        })
        .collect();
    if !due.is_empty() {
        // One line per pass naming each project read and why, with the
        // generation that asked for it.
        eprintln!(
            "{}",
            serde_json::json!({
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
            })
        );
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
                    read_at: now,
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
    authentication: &Result<(), String>,
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
                unavailable_reason: Some(reason.clone()),
                ..GithubStatusSnapshot::default()
            },
            ..GithubProjectSnapshot::default()
        },
        Ok(()) => read_project(&main, root_path),
    };
    // A failure and an empty answer are both stated, separately: an empty
    // list with no reason is a repository with no pull requests.
    eprintln!(
        "{}",
        serde_json::json!({
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
        })
    );
    project
}

/// Whether `gh` is usable at all, as the one sentence the card may show.
fn authentication() -> Result<(), String> {
    match Command::new("gh").args(["auth", "status"]).output() {
        Err(_) => {
            Err("gh is not installed. Install the GitHub CLI to see pull requests.".to_owned())
        }
        Ok(output) if output.status.success() => Ok(()),
        Ok(_) => Err("gh is not logged in. Run `gh auth login` to see pull requests.".to_owned()),
    }
}

fn read_project(root: &Path, root_path: String) -> GithubProjectSnapshot {
    let default_branch = match gh(
        root,
        &[
            "repo",
            "view",
            "--json",
            "defaultBranchRef",
            "--jq",
            ".defaultBranchRef.name",
        ],
    ) {
        Ok(output) => {
            let branch = output.trim().to_owned();
            (!branch.is_empty()).then_some(branch)
        }
        Err(reason) => {
            return GithubProjectSnapshot {
                root_path,
                status: failed(reason),
                ..GithubProjectSnapshot::default()
            };
        }
    };

    let listed = match gh(
        root,
        &[
            "pr",
            "list",
            "--state",
            "all",
            "--limit",
            PULL_REQUEST_LIMIT,
            "--json",
            "number,headRefName,baseRefName,state,reviewDecision,isDraft,url,mergedAt,updatedAt",
        ],
    ) {
        Ok(listed) => listed,
        Err(reason) => {
            return GithubProjectSnapshot {
                root_path,
                status: failed(reason),
                default_branch,
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
            },
            default_branch,
            pull_requests,
        },
        // A response shape Hide cannot read is a failure with a reason, never
        // an empty pull-request list: "no pull requests" is a real answer and
        // must not be manufactured out of a parse error.
        Err(reason) => GithubProjectSnapshot {
            root_path,
            status: failed(reason),
            default_branch,
            ..GithubProjectSnapshot::default()
        },
    }
}

fn failed(reason: String) -> GithubStatusSnapshot {
    GithubStatusSnapshot {
        available: true,
        loading: false,
        stale: true,
        last_success_at_unix_ms: None,
        unavailable_reason: Some(reason),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPullRequest {
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
    let leap = |year: u64| year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
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

fn gh(cwd: &Path, arguments: &[&str]) -> Result<String, String> {
    // `gh` has no `-C`: it finds the repository from the working directory,
    // so the directory is set on the child rather than passed as a flag. The
    // git-shaped spelling was accepted by the compiler and rejected by `gh`
    // at runtime with "unknown shorthand flag: 'C'".
    let output = Command::new("gh")
        .current_dir(cwd)
        .args(arguments)
        .output()
        .map_err(|error| format!("gh could not be run: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!(
                "gh {} {} exited with {}",
                arguments[0], arguments[1], output.status
            )
        } else {
            format!("gh {} {}: {stderr}", arguments[0], arguments[1])
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_is_read_first_then_only_when_its_own_request_moves_or_ages() {
        assert_eq!(read_reason(None, 0), Some(ReadReason::First));
        assert_eq!(
            read_reason(Some((0, Duration::from_secs(1))), 1),
            Some(ReadReason::Generation)
        );
        assert_eq!(
            read_reason(Some((1, REFRESH_INTERVAL)), 1),
            Some(ReadReason::Interval)
        );
        assert_eq!(read_reason(Some((1, Duration::from_secs(1))), 1), None);
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
            r#"{{"number":{number},"headRefName":"{branch}","baseRefName":"main","state":"{state}","reviewDecision":{review},"isDraft":{draft},"url":"https://example.invalid/{number}","mergedAt":null,"updatedAt":"{updated}"}}"#,
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
