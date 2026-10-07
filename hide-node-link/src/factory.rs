//! What the Software Factory has the core's own node do (PRD core-host-node
//! D-01, D-21): the Factory decides on the core's machine, and its machine
//! work is that machine's node's. A git or `gh` command is a typed shape the
//! node turns into one fixed command line ([`FactoryGit::args`],
//! [`FactoryGh::args`]), so a request names values, never arguments; the
//! project's quick check and verify bundle are the operator's own command
//! text, run under the node's shell. The node runs and reads; what an answer
//! means for a Task stays the Factory's.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::gh::is_repository;
use crate::git::{branch_name, revision_name};

/// The longest one git or `gh` command may run on the node.
pub const RUN_DEADLINE_MS: u64 = 120_000;
/// The most runs a node's verify queue holds waiting; a full queue refuses
/// the next one.
pub const VERIFY_QUEUE_LIMIT: usize = 256;
/// The most commands one verify bundle runs.
pub const VERIFY_COMMAND_LIMIT: usize = 32;
/// The longest command text a quick check or a bundle command may be.
pub const COMMAND_TEXT_LIMIT: usize = 16 * 1024;
/// The longest title, and the longest body, a `gh` write may carry.
pub const TITLE_LIMIT: usize = 1024;
pub const BODY_LIMIT: usize = 256 * 1024;
/// The most names a project read answers from the project's top folder.
pub const PROJECT_ENTRY_LIMIT: usize = 200;
/// The most of one project file a project read answers.
pub const PROJECT_READ_LIMIT: usize = 256 * 1024;
/// The end of a verify log a poll or a log read answers.
pub const LOG_TAIL_LIMIT: usize = 64 * 1024;
/// The largest PRD a Task may attach.
pub const PRD_LIMIT: u64 = 4 * 1024 * 1024;

/// The files a project read answers the text of: the guides a judgment
/// reads, and the two the probe reads verify candidates from.
pub const PROJECT_READS: [&str; 5] = [
    "AGENTS.md",
    "CLAUDE.md",
    "README.md",
    "package.json",
    "Makefile",
];
/// The files whose presence alone names a verify candidate.
pub const PROJECT_MARKERS: [&str; 3] = ["Cargo.toml", "pnpm-lock.yaml", "pyproject.toml"];

/// One Factory request to its node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "factory", rename_all = "snake_case")]
pub enum FactoryCall {
    /// One git command in `cwd`; answers a [`RunAnswer`].
    Git { cwd: String, command: FactoryGit },
    /// One `gh` command with the operator's login; answers a [`RunAnswer`].
    Gh { command: FactoryGh },
    /// The project's quick check, `text`, under the node's shell in `cwd`;
    /// answers a [`RunAnswer`].
    Check { cwd: String, text: String },
    /// Queues a verify bundle; answers the path of its log. The same id is
    /// never queued twice.
    VerifySubmit { job: VerifyJob },
    /// Advances the node's verify queue and answers `id`'s
    /// [`VerifyOutcome`].
    VerifyPoll { id: String },
    /// Whether `id` is queued, running or finished; answers a bool.
    VerifyKnown { id: String },
    /// Forgets `id`'s finished result, so the same id can run again.
    VerifyForget { id: String },
    /// Ends `id`, queued or running.
    VerifyCancel { id: String },
    /// Ends every run queued or running for the core: the Factory that
    /// asked for them is closing.
    VerifyClose,
    /// The end of a verify log the node wrote; answers `Option<String>`, at
    /// most [`LOG_TAIL_LIMIT`] bytes.
    LogTail { path: String },
    /// What the probe and a judgment read from a project's top folder;
    /// answers [`ProjectFiles`].
    Project { path: String },
    /// The PRD file a Task attaches, at most [`PRD_LIMIT`] bytes; answers
    /// its bytes in base64.
    ReadPrd { path: String },
    /// The repository a linked worktree at `checkout` belongs to, read
    /// before it goes; answers `Option<String>`, `None` for a folder already
    /// gone.
    WorktreeRoot { checkout: String },
    /// Removes the worktree at `checkout` from the repository at `root`,
    /// forced only when `discard`; a discarded one's `branch` is deleted
    /// with it.
    RemoveWorktree {
        root: String,
        checkout: String,
        branch: String,
        discard: bool,
    },
    /// The machine's memory pressure as the system reports it; answers
    /// [`MemoryPressure`].
    MemoryPressure,
    /// The `hide` program beside the node's own program; answers
    /// `Option<String>`.
    HideProgram,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPressure {
    Normal,
    Warn,
    Critical,
}

/// How one git, `gh` or check run ended.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RunAnswer {
    Finished {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    /// Past [`RUN_DEADLINE_MS`]; the node ended it.
    TimedOut,
    /// The caller asked it to stop.
    Stopped,
    /// It could not start or be waited on.
    Unstarted { reason: String },
}

/// One verify bundle, run in its own process group with a time cap over the
/// whole bundle (D-46, D-53).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VerifyJob {
    pub id: String,
    /// The folder the run's log is written in.
    pub logs: String,
    pub cwd: String,
    pub commands: Vec<String>,
    /// Git steps run first, in order; a failed step ends the run before any
    /// command.
    pub prepare: Vec<VerifyStep>,
    pub timeout_ms: u64,
    /// The most output the bundle may write before it is ended.
    pub output_limit: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VerifyStep {
    pub cwd: String,
    pub command: FactoryGit,
}

/// Where a verify run stands, as the node saw it; the Factory reads what a
/// failure means.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum VerifyOutcome {
    /// Queued, running, or never submitted.
    Pending,
    Passed,
    /// A prepare step failed, named as a command line.
    StepFailed {
        step: String,
        answer: RunAnswer,
    },
    /// A command exited unsuccessfully; `tail` is the end of its log.
    Failed {
        command: String,
        code: Option<i32>,
        tail: String,
        log: String,
    },
    /// The bundle ran past its time cap.
    TimedOut {
        command: String,
        minutes: u64,
    },
    /// The bundle wrote past its output cap.
    OverOutput {
        command: String,
        log: String,
    },
    /// The log could not be opened; `disk_full` when the volume is full.
    LogUnwritable {
        disk_full: bool,
        error: String,
    },
    /// A command could not start or be waited on.
    Unstarted {
        command: String,
        error: String,
    },
}

/// What the probe and a judgment read from a project's top folder.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectFiles {
    /// Names in the folder, at most [`PROJECT_ENTRY_LIMIT`], sorted.
    pub entries: Vec<String>,
    /// Each of [`PROJECT_READS`] that is a readable file, cut at
    /// [`PROJECT_READ_LIMIT`].
    pub texts: BTreeMap<String, String>,
    /// Each of [`PROJECT_MARKERS`] that is there.
    pub markers: Vec<String>,
}

/// One git command the Factory runs, each a fixed command line.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "git", rename_all = "snake_case")]
pub enum FactoryGit {
    ShowToplevel,
    AbbrevHead,
    OriginUrl,
    /// Tracked changes only.
    TrackedStatus,
    /// Fetches `branch` from `origin`.
    Fetch {
        branch: String,
    },
    /// Adds a detached worktree at `path` on `branch`.
    WorktreeAdd {
        path: String,
        branch: String,
    },
    CheckoutDetached {
        revision: String,
    },
    RevParse {
        revision: String,
    },
    RemoteHead {
        branch: String,
    },
    /// Drops `origin`'s stale tracking ref for `branch`.
    DropRemoteRef {
        branch: String,
    },
    /// Publishes HEAD as `branch`, leased against the tracking ref.
    PushLeased {
        branch: String,
    },
    /// Publishes HEAD as `branch`, replacing what is there.
    PushForced {
        branch: String,
    },
    DiffNumstat {
        base: String,
    },
    DiffNames {
        base: String,
    },
    DiffPatch {
        base: String,
    },
    /// Answers 1 with the conflicted files when HEAD does not merge into
    /// `base` cleanly.
    MergeTree {
        base: String,
    },
    IsAncestor {
        revision: String,
    },
    Merge {
        message: String,
        revision: String,
    },
    MergeQuiet {
        revision: String,
    },
    MergeFastForward {
        revision: String,
    },
    MergeAbort,
    Parents {
        revision: String,
    },
    Revert {
        revision: String,
        mainline: bool,
    },
    RevertAbort,
}

impl FactoryGit {
    /// The command line after `git`, or why the values cannot make one.
    pub fn args(&self) -> Result<Vec<String>, String> {
        let owned = |args: &[&str]| Ok(args.iter().map(|arg| (*arg).to_owned()).collect());
        match self {
            Self::ShowToplevel => owned(&["rev-parse", "--show-toplevel"]),
            Self::AbbrevHead => owned(&["rev-parse", "--abbrev-ref", "HEAD"]),
            Self::OriginUrl => owned(&["remote", "get-url", "origin"]),
            Self::TrackedStatus => owned(&["status", "--porcelain", "--untracked-files=no"]),
            Self::Fetch { branch } => owned(&["fetch", "--quiet", "origin", branch_name(branch)?]),
            Self::WorktreeAdd { path, branch } => owned(&[
                "worktree",
                "add",
                "--detach",
                absolute(path)?,
                branch_name(branch)?,
            ]),
            Self::CheckoutDetached { revision } => {
                owned(&["checkout", "--quiet", "--detach", revision_name(revision)?])
            }
            Self::RevParse { revision } => owned(&["rev-parse", revision_name(revision)?]),
            Self::RemoteHead { branch } => owned(&[
                "ls-remote",
                "--heads",
                "origin",
                &format!("refs/heads/{}", branch_name(branch)?),
            ]),
            Self::DropRemoteRef { branch } => owned(&[
                "update-ref",
                "-d",
                &format!("refs/remotes/origin/{}", branch_name(branch)?),
            ]),
            Self::PushLeased { branch } => owned(&[
                "push",
                "--quiet",
                "--force-with-lease",
                "--set-upstream",
                "origin",
                &format!("HEAD:refs/heads/{}", branch_name(branch)?),
            ]),
            Self::PushForced { branch } => owned(&[
                "push",
                "--quiet",
                "--force",
                "origin",
                &format!("HEAD:refs/heads/{}", branch_name(branch)?),
            ]),
            Self::DiffNumstat { base } => owned(&["diff", "--numstat", &range(base)?]),
            Self::DiffNames { base } => owned(&["diff", "--name-only", &range(base)?]),
            Self::DiffPatch { base } => owned(&["diff", "--stat", "--patch", &range(base)?]),
            Self::MergeTree { base } => owned(&[
                "merge-tree",
                "--write-tree",
                "--name-only",
                "--no-messages",
                revision_name(base)?,
                "HEAD",
            ]),
            Self::IsAncestor { revision } => owned(&[
                "merge-base",
                "--is-ancestor",
                revision_name(revision)?,
                "HEAD",
            ]),
            Self::Merge { message, revision } => owned(&[
                "merge",
                "--no-ff",
                "--no-edit",
                "-m",
                bounded(message, TITLE_LIMIT, "merge message")?,
                revision_name(revision)?,
            ]),
            Self::MergeQuiet { revision } => {
                owned(&["merge", "--no-edit", "--quiet", revision_name(revision)?])
            }
            Self::MergeFastForward { revision } => {
                owned(&["merge", "--ff-only", revision_name(revision)?])
            }
            Self::MergeAbort => owned(&["merge", "--abort"]),
            Self::Parents { revision } => {
                owned(&["rev-list", "--parents", "-n", "1", revision_name(revision)?])
            }
            Self::Revert { revision, mainline } => {
                let revision = revision_name(revision)?;
                if *mainline {
                    owned(&["revert", "--no-edit", "-m", "1", revision])
                } else {
                    owned(&["revert", "--no-edit", revision])
                }
            }
            Self::RevertAbort => owned(&["revert", "--abort"]),
        }
    }
}

/// How a pull request is merged.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GhMergeMethod {
    Merge,
    Squash,
    Rebase,
}

/// The fields a pull request lookup by head branch asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrFindFields {
    /// A Task's pull request.
    Task,
    /// A revert's pull request.
    Revert,
}

/// The label every Factory Task's issue carries.
pub const LABEL: &str = "factory";

/// One `gh` command the Factory runs, each a fixed command line.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "gh", rename_all = "snake_case")]
pub enum FactoryGh {
    /// The repository behind the `origin` URL `remote`.
    RepoView {
        remote: String,
    },
    User,
    RequiredChecks {
        repo: String,
        branch: String,
    },
    LabelCreate {
        repo: String,
    },
    /// Issues whose body carries `marker`.
    IssueFind {
        repo: String,
        marker: String,
    },
    IssueCreate {
        repo: String,
        title: String,
        body: String,
    },
    IssueLabel {
        repo: String,
        number: u64,
    },
    IssueView {
        repo: String,
        number: u64,
    },
    /// One issue with its labels.
    IssueLabels {
        repo: String,
        number: u64,
    },
    /// The newest labelled issues.
    LabelledIssues {
        repo: String,
    },
    /// The newest pull requests with the issues each closes.
    RecentPrs {
        repo: String,
    },
    CheckRuns {
        repo: String,
        sha: String,
    },
    PrView {
        repo: String,
        number: u64,
    },
    PrFind {
        repo: String,
        branch: String,
        fields: PrFindFields,
    },
    PrCreate {
        repo: String,
        branch: String,
        base: String,
        title: String,
        body: String,
    },
    PrClose {
        repo: String,
        number: u64,
    },
    PrReopen {
        repo: String,
        number: u64,
    },
    PrMerge {
        repo: String,
        number: u64,
        method: GhMergeMethod,
        head: String,
    },
    WorkflowRuns {
        repo: String,
        sha: String,
    },
    RunRerun {
        repo: String,
        run: u64,
    },
}

impl FactoryGh {
    /// The command line after `gh`, or why the values cannot make one.
    pub fn args(&self) -> Result<Vec<String>, String> {
        let owned = |args: &[&str]| Ok(args.iter().map(|arg| (*arg).to_owned()).collect());
        match self {
            Self::RepoView { remote } => owned(&[
                "repo",
                "view",
                remote_url(remote)?,
                "--json",
                "nameWithOwner,defaultBranchRef,mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed",
            ]),
            Self::User => owned(&["api", "user"]),
            Self::RequiredChecks { repo, branch } => owned(&[
                "api",
                &format!(
                    "repos/{}/branches/{}/protection/required_status_checks",
                    repository(repo)?,
                    path_segment(branch)?
                ),
            ]),
            Self::LabelCreate { repo } => owned(&[
                "label",
                "create",
                LABEL,
                "--repo",
                repository(repo)?,
                "--force",
                "--color",
                "5319e7",
                "--description",
                "Software Factory Task",
            ]),
            Self::IssueFind { repo, marker } => owned(&[
                "issue",
                "list",
                "--repo",
                repository(repo)?,
                "--state",
                "all",
                "--search",
                &format!("\"{}\" in:body", bounded(marker, TITLE_LIMIT, "marker")?),
                "--json",
                "number,body",
            ]),
            Self::IssueCreate { repo, title, body } => owned(&[
                "issue",
                "create",
                "--repo",
                repository(repo)?,
                "--title",
                bounded(title, TITLE_LIMIT, "title")?,
                "--body",
                bounded(body, BODY_LIMIT, "body")?,
                "--label",
                LABEL,
            ]),
            Self::IssueLabel { repo, number } => owned(&[
                "issue",
                "edit",
                &number.to_string(),
                "--repo",
                repository(repo)?,
                "--add-label",
                LABEL,
            ]),
            Self::IssueView { repo, number } => owned(&[
                "issue",
                "view",
                &number.to_string(),
                "--repo",
                repository(repo)?,
                "--json",
                "title,body,state",
            ]),
            Self::IssueLabels { repo, number } => owned(&[
                "issue",
                "view",
                &number.to_string(),
                "--repo",
                repository(repo)?,
                "--json",
                "number,title,body,state,labels",
            ]),
            Self::LabelledIssues { repo } => owned(&[
                "issue",
                "list",
                "--repo",
                repository(repo)?,
                "--label",
                LABEL,
                "--state",
                "all",
                "--limit",
                "200",
                "--json",
                "number,title,body,state",
            ]),
            Self::RecentPrs { repo } => owned(&[
                "pr",
                "list",
                "--repo",
                repository(repo)?,
                "--state",
                "all",
                "--limit",
                "100",
                "--json",
                "number,url,state,headRefName,isCrossRepository,closingIssuesReferences",
            ]),
            Self::CheckRuns { repo, sha } => owned(&[
                "api",
                &format!(
                    "repos/{}/commits/{}/check-runs?per_page=100",
                    repository(repo)?,
                    commit(sha)?
                ),
            ]),
            Self::PrView { repo, number } => owned(&[
                "pr",
                "view",
                &number.to_string(),
                "--repo",
                repository(repo)?,
                "--json",
                "number,url,state,headRefName,headRefOid,mergeCommit",
            ]),
            Self::PrFind {
                repo,
                branch,
                fields,
            } => owned(&[
                "pr",
                "list",
                "--repo",
                repository(repo)?,
                "--head",
                branch_name(branch)?,
                "--state",
                "open",
                "--json",
                match fields {
                    PrFindFields::Task => "number,url,state,headRefName,isCrossRepository",
                    PrFindFields::Revert => "number,isCrossRepository",
                },
            ]),
            Self::PrCreate {
                repo,
                branch,
                base,
                title,
                body,
            } => owned(&[
                "pr",
                "create",
                "--repo",
                repository(repo)?,
                "--head",
                branch_name(branch)?,
                "--base",
                branch_name(base)?,
                "--title",
                bounded(title, TITLE_LIMIT, "title")?,
                "--body",
                bounded(body, BODY_LIMIT, "body")?,
            ]),
            Self::PrClose { repo, number } => owned(&[
                "pr",
                "close",
                &number.to_string(),
                "--repo",
                repository(repo)?,
            ]),
            Self::PrReopen { repo, number } => owned(&[
                "pr",
                "reopen",
                &number.to_string(),
                "--repo",
                repository(repo)?,
            ]),
            Self::PrMerge {
                repo,
                number,
                method,
                head,
            } => owned(&[
                "pr",
                "merge",
                &number.to_string(),
                "--repo",
                repository(repo)?,
                match method {
                    GhMergeMethod::Merge => "--merge",
                    GhMergeMethod::Squash => "--squash",
                    GhMergeMethod::Rebase => "--rebase",
                },
                "--match-head-commit",
                commit(head)?,
            ]),
            Self::WorkflowRuns { repo, sha } => owned(&[
                "api",
                &format!(
                    "repos/{}/actions/runs?head_sha={}&per_page=50",
                    repository(repo)?,
                    commit(sha)?
                ),
            ]),
            Self::RunRerun { repo, run } => owned(&[
                "run",
                "rerun",
                &run.to_string(),
                "--repo",
                repository(repo)?,
            ]),
        }
    }
}

/// `base...HEAD`, the changes HEAD made since it left `base`.
fn range(base: &str) -> Result<String, String> {
    Ok(format!("{}...HEAD", revision_name(base)?))
}

/// A full or abbreviated commit id.
fn commit(value: &str) -> Result<&str, String> {
    (value.len() >= 4 && value.len() <= 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then_some(value)
        .ok_or_else(|| format!("not a commit: {value:?}"))
}

/// One segment of a GitHub API path: a branch name that cannot add a query
/// or leave its place in the path.
fn path_segment(value: &str) -> Result<&str, String> {
    let value = branch_name(value)?;
    value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"._/-".contains(&b))
        .then_some(value)
        .ok_or_else(|| format!("not a plain branch: {value:?}"))
}

fn repository(value: &str) -> Result<&str, String> {
    is_repository(value)
        .then_some(value)
        .ok_or_else(|| format!("not a repository: {value:?}"))
}

/// The `origin` URL as git answered it.
fn remote_url(value: &str) -> Result<&str, String> {
    (!value.is_empty()
        && value.len() <= 2048
        && !value.starts_with('-')
        && value.chars().all(|c| !c.is_control() && !c.is_whitespace()))
    .then_some(value)
    .ok_or_else(|| format!("not a remote URL: {value:?}"))
}

fn absolute(value: &str) -> Result<&str, String> {
    (value.starts_with('/') || std::path::Path::new(value).is_absolute())
        .then_some(value)
        .ok_or_else(|| format!("not an absolute path: {value:?}"))
}

fn bounded<'a>(value: &'a str, limit: usize, what: &str) -> Result<&'a str, String> {
    (value.len() <= limit)
        .then_some(value)
        .ok_or_else(|| format!("the {what} is longer than {limit} bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value never becomes an option or a second argument: git and `gh`
    /// read each of these as the one thing its place names.
    #[test]
    fn a_value_that_would_read_as_an_option_or_a_range_is_refused() {
        for revision in ["-n", "--upload-pack=x", "a..b", "a:b", "", "a b", "a\nb"] {
            assert!(
                FactoryGit::RevParse {
                    revision: revision.into()
                }
                .args()
                .is_err(),
                "{revision:?}"
            );
        }
        for branch in ["/x", "x/", "-x", "a:b"] {
            assert!(
                FactoryGit::PushLeased {
                    branch: branch.into()
                }
                .args()
                .is_err(),
                "{branch:?}"
            );
        }
        assert!(
            FactoryGh::CheckRuns {
                repo: "o/r".into(),
                sha: "abc?x=1".into()
            }
            .args()
            .is_err()
        );
        assert!(
            FactoryGh::RequiredChecks {
                repo: "o/r".into(),
                branch: "main?per_page=1".into()
            }
            .args()
            .is_err()
        );
        assert!(
            FactoryGh::PrClose {
                repo: "../r".into(),
                number: 1
            }
            .args()
            .is_err()
        );
        assert!(
            FactoryGh::RepoView {
                remote: "--jq=.".into()
            }
            .args()
            .is_err()
        );
        assert!(
            FactoryGit::WorktreeAdd {
                path: "relative".into(),
                branch: "main".into()
            }
            .args()
            .is_err()
        );
    }

    /// The command lines are the ones the Factory ran before they moved to
    /// the node, word for word.
    #[test]
    fn each_shape_is_its_fixed_command_line() {
        assert_eq!(
            FactoryGit::PushLeased {
                branch: "factory/1-x".into()
            }
            .args()
            .unwrap(),
            [
                "push",
                "--quiet",
                "--force-with-lease",
                "--set-upstream",
                "origin",
                "HEAD:refs/heads/factory/1-x"
            ]
        );
        assert_eq!(
            FactoryGit::Revert {
                revision: "abcd".into(),
                mainline: true
            }
            .args()
            .unwrap(),
            ["revert", "--no-edit", "-m", "1", "abcd"]
        );
        assert_eq!(
            FactoryGit::DiffNames {
                base: "origin/main".into()
            }
            .args()
            .unwrap(),
            ["diff", "--name-only", "origin/main...HEAD"]
        );
        assert_eq!(
            FactoryGh::PrMerge {
                repo: "o/r".into(),
                number: 7,
                method: GhMergeMethod::Squash,
                head: "abcdef".into()
            }
            .args()
            .unwrap(),
            [
                "pr",
                "merge",
                "7",
                "--repo",
                "o/r",
                "--squash",
                "--match-head-commit",
                "abcdef"
            ]
        );
        assert_eq!(
            FactoryGh::IssueFind {
                repo: "o/r".into(),
                marker: "<!-- hide-factory: f/t -->".into()
            }
            .args()
            .unwrap(),
            [
                "issue",
                "list",
                "--repo",
                "o/r",
                "--state",
                "all",
                "--search",
                "\"<!-- hide-factory: f/t -->\" in:body",
                "--json",
                "number,body"
            ]
        );
        // A title or a body is the value of its flag, whatever it starts with.
        assert_eq!(
            FactoryGh::IssueCreate {
                repo: "o/r".into(),
                title: "-x".into(),
                body: "--y".into()
            }
            .args()
            .unwrap(),
            [
                "issue", "create", "--repo", "o/r", "--title", "-x", "--body", "--y", "--label",
                "factory"
            ]
        );
    }
}
