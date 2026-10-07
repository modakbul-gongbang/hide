//! The `gh` commands the core may have its own node run with the operator's
//! GitHub login, and what one answers. The node refuses any other command
//! line ([`allowed`]); the fields and caps below are the only ones a command
//! may name, and the core builds its commands from them.

use serde::{Deserialize, Serialize};

/// Why a GitHub read failed, as a code the screen words (the reason stays
/// `gh`'s own stderr, which is data).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubFailureCategory {
    NotInstalled,
    NotLoggedIn,
    NoGithubRemote,
    NetworkOrRateLimit,
}

impl GithubFailureCategory {
    /// The words for diagnostics and the errors a command reports.
    pub fn english(self) -> &'static str {
        match self {
            Self::NotInstalled => "not installed",
            Self::NotLoggedIn => "not logged in",
            Self::NoGithubRemote => "no GitHub remote",
            Self::NetworkOrRateLimit => "network or rate limit",
        }
    }
}

/// Whether `value` reads as `owner/name`: two plain path-safe parts, so it
/// can be handed to `gh` as a repository.
pub fn is_repository(value: &str) -> bool {
    let mut parts = value.split('/');
    let valid = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    };
    parts.next().is_some_and(valid) && parts.next().is_some_and(valid) && parts.next().is_none()
}

/// The fields `issue_detail` asks `gh issue view` for, and the only ones
/// `run_gh` lets it ask for.
pub const ISSUE_DETAIL_FIELDS: &str = "body,labels,author,assignees,comments,createdAt";

/// The fields `search` asks `gh search prs` and `gh search issues` for, and
/// the only ones `run_gh` lets them ask for. The search has no head branch, and
/// only a pull request has `isDraft`.
pub const SEARCH_PR_FIELDS: &str = "isDraft,number,repository,state,title,url";

pub const SEARCH_ISSUE_FIELDS: &str = "number,repository,state,title,url";

/// Most pull requests, and most issues, one search returns in all, and what
/// one `gh search` is asked for per repository.
pub const SEARCH_LIMIT: usize = 20;

/// Longest query, in characters, a search takes.
pub const SEARCH_QUERY_LIMIT: usize = 200;

/// Most repositories one search names (`runtime/issues.rs` caps the projects at the same number).
pub const SEARCH_REPOSITORY_LIMIT: usize = 20;

/// A query as `run_gh` lets it reach `gh`: some text, within the cap.
pub fn is_search_query(query: &str) -> bool {
    !query.trim().is_empty() && query.chars().count() <= SEARCH_QUERY_LIMIT
}

/// The one shape of `gh search prs|issues` Hide runs: `search <kind>`, one
/// `--repo owner/name` per repository, the cap, that kind's fixed fields, then
/// `--` and the query's words, each of which is just text to `gh`.
pub fn is_search_call(arguments: &[&str]) -> bool {
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
        if !is_repository(repository) {
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

/// The fields `pr_feedback` asks `gh pr view` for, and the only ones
/// `run_gh` lets it ask for.
pub const PR_FEEDBACK_FIELDS: &str = "body,statusCheckRollup,reviews";

/// A pull request or issue number as `gh` takes it.
pub fn is_number(argument: &str) -> bool {
    !argument.is_empty() && argument.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether `arguments` is one of the `gh` command lines Hide runs: reads,
/// and the two writes (docs/ARCHITECTURE.md), a new issue with a title and a
/// body, and a pull request's body, nothing else of either.
pub fn allowed(arguments: &[&str]) -> bool {
    arguments.starts_with(&["auth", "status"])
        || arguments.starts_with(&["pr", "list"])
        || arguments.starts_with(&["issue", "list"])
        || arguments == ["repo", "view", "--json", "nameWithOwner"]
        || arguments == ["repo", "view", "--json", "nameWithOwner,id"]
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
        // The search: the registered repositories, the cap, fixed fields, and
        // the query's words after `--` so none can be read as a flag.
        || is_search_call(arguments)
        || (arguments.len() == 4
            && arguments[..3] == ["api", "graphql", "-f"]
            && (arguments[3].starts_with("query=query HideLinkedIssues {")
                || arguments[3].starts_with("query=query HideIssueDependencies {")))
}

/// What one `gh` command answered: its output, or why it gave none.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum GhAnswer {
    Output {
        stdout: String,
    },
    Failed {
        category: GithubFailureCategory,
        reason: String,
    },
}
