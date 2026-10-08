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

/// What the list of every pull request asks for. `statusCheckRollup` is
/// left out: asking GitHub for the checks of all 200 pull requests, merged
/// and closed ones included, is what made one read take 11 - 14 seconds
/// against the 15-second limit, and nothing draws a settled pull request's
/// checks as news.
pub const PULL_REQUEST_FIELDS: &str = "number,title,headRefName,headRefOid,isCrossRepository,baseRefName,state,reviewDecision,isDraft,url,mergedAt,updatedAt,createdAt,closedAt,closingIssuesReferences";

/// The second list: only the open pull requests, only their checks.
pub const OPEN_CHECK_FIELDS: &str = "number,statusCheckRollup";

/// Every pull request `gh` will return in one call. Past this, older pull
/// requests are simply absent and their branches read as having none; the
/// limit is stated here so the risk is findable from the code that takes it.
pub const PULL_REQUEST_LIMIT: &str = "200";

/// The open issues one read lists, one past what the screen shows, so the
/// last proves there are more.
pub const ISSUE_LIST_LIMIT: &str = "201";
pub const ISSUE_LIST_SEARCH: &str = "sort:updated-desc";
pub const ISSUE_LIST_FIELDS: &str = "number,title,url,state,labels,updatedAt,createdAt,closedAt";
/// The same list with the projects each issue is in.
pub const ISSUE_LIST_PROJECT_FIELDS: &str =
    "number,title,url,state,labels,projectItems,updatedAt,createdAt,closedAt";

/// The merged pull requests of one branch a worktree cleanup reads as proof
/// the branch landed.
pub const MERGED_PROOF_LIMIT: &str = "100";
pub const MERGED_PROOF_FIELDS: &str = "headRefOid,baseRefName";

/// A value `gh` reads as the value of the flag before it, never a flag.
fn is_value(argument: &str) -> bool {
    !argument.is_empty() && !argument.starts_with('-')
}

/// A pull request or issue number as `gh` takes it.
pub fn is_number(argument: &str) -> bool {
    !argument.is_empty() && argument.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether `arguments` is one of the `gh` command lines Hide runs: reads,
/// and the two writes (docs/ARCHITECTURE.md), a new issue with a title and a
/// body, and a pull request's body, nothing else of either.
pub fn allowed(arguments: &[&str]) -> bool {
    arguments == ["auth", "status"]
        || matches!(
            arguments,
            ["pr", "list", "--state", "open", "--limit", PULL_REQUEST_LIMIT, "--json", OPEN_CHECK_FIELDS]
                | ["pr", "list", "--state", "all", "--limit", PULL_REQUEST_LIMIT, "--json", PULL_REQUEST_FIELDS]
        )
        || matches!(
            arguments,
            ["pr", "list", "--state", "merged", "--head", branch, "--limit", MERGED_PROOF_LIMIT, "--json", MERGED_PROOF_FIELDS]
                if is_value(branch)
        )
        || matches!(
            arguments,
            ["issue", "list", "--state", "open", "--limit", ISSUE_LIST_LIMIT, "--search", ISSUE_LIST_SEARCH, "--json", fields]
                if *fields == ISSUE_LIST_FIELDS || *fields == ISSUE_LIST_PROJECT_FIELDS
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_takes_its_repositories_the_cap_fixed_fields_and_the_query_words_after_the_dashes() {
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

    /// The list reads are their exact command lines: no other state, limit,
    /// field list or flag, and a branch that would read as a flag is refused.
    #[test]
    fn a_list_read_is_its_exact_command_line() {
        let pr_list = |state: &'static str, fields: &'static str| {
            vec![
                "pr",
                "list",
                "--state",
                state,
                "--limit",
                PULL_REQUEST_LIMIT,
                "--json",
                fields,
            ]
        };
        let merged = |branch: &'static str| {
            vec![
                "pr",
                "list",
                "--state",
                "merged",
                "--head",
                branch,
                "--limit",
                MERGED_PROOF_LIMIT,
                "--json",
                MERGED_PROOF_FIELDS,
            ]
        };
        let issues = |fields: &'static str| {
            vec![
                "issue",
                "list",
                "--state",
                "open",
                "--limit",
                ISSUE_LIST_LIMIT,
                "--search",
                ISSUE_LIST_SEARCH,
                "--json",
                fields,
            ]
        };
        for arguments in [
            vec!["auth", "status"],
            pr_list("open", OPEN_CHECK_FIELDS),
            pr_list("all", PULL_REQUEST_FIELDS),
            merged("factory/1-task"),
            issues(ISSUE_LIST_FIELDS),
            issues(ISSUE_LIST_PROJECT_FIELDS),
        ] {
            assert!(allowed(&arguments), "{arguments:?} must be allowed");
        }
        let mut with_web = pr_list("all", PULL_REQUEST_FIELDS);
        with_web.push("--web");
        let mut token = vec!["auth", "status"];
        token.push("--show-token");
        for arguments in [
            token,
            with_web,
            pr_list("closed", PULL_REQUEST_FIELDS),
            pr_list("open", PULL_REQUEST_FIELDS),
            pr_list("all", "body"),
            merged("--web"),
            merged(""),
            issues("body"),
            vec!["issue", "list"],
            vec!["pr", "list"],
        ] {
            assert!(!allowed(&arguments), "{arguments:?} must be refused");
        }
    }
}
