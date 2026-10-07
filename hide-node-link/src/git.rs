//! The git shapes a request carries.

use serde::{Deserialize, Serialize};

/// The further diffs one read answers at most: one per View area the shell
/// can show side by side (PRD S7 A5), so a read stays bounded whoever asks.
pub const MAX_DIFFS: usize = 6;

/// A file whose diff a View display shows, relative to the scope like every
/// path in the answer, and the group it is taken in.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiffTarget {
    pub path: String,
    pub committed: bool,
}

/// What to read: the folder below the root the answer is limited to, the
/// file whose diff to fetch and which group it is in, and the branch the
/// committed group is measured against. `base: None` measures against the
/// repository's default branch when it has one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangesQuery {
    pub scope: String,
    pub selected: Option<String>,
    pub committed: bool,
    pub base: Option<String>,
    /// Further diffs to take in the same read, one per View display of a
    /// diff; past `MAX_DIFFS` they are not answered.
    #[serde(default)]
    pub diffs: Vec<DiffTarget>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Changes {
    /// The working tree against `HEAD`: what `git status` reports.
    pub entries: Vec<ChangedFile>,
    /// What this branch's commits changed since `base`. Absent when there is
    /// no base to compare with, which is not the same as an empty group.
    pub committed: Option<Vec<ChangedFile>>,
    /// The base the committed group was measured against, as this checkout
    /// resolved it.
    pub base: Option<String>,
    pub diff: Option<Diff>,
    /// One diff per answered target of `ChangesQuery::diffs`, in order. A
    /// target no longer in its group is answered with empty text and a
    /// notice saying so, never left out.
    #[serde(default)]
    pub diffs: Vec<GroupDiff>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupDiff {
    pub committed: bool,
    #[serde(flatten)]
    pub diff: Diff,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    pub previous: Option<String>,
    pub status: FileStatus,
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diff {
    pub path: String,
    pub text: String,
    /// Set when the diff was cut short or could not be taken, naming why.
    pub notice: Option<String>,
}

/// The six working-tree states the view presents. Git's porcelain codes
/// carry more distinctions; [`FileStatus::from_porcelain`] is the single
/// place they collapse.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Untracked,
    Renamed,
    Conflict,
}

impl FileStatus {
    /// Maps one porcelain v1 `XY` pair onto the presented status. Index and
    /// worktree columns are read together: a file staged as added and then
    /// edited is still an addition to the reader, and a delete on either side
    /// is a delete.
    pub fn from_porcelain(code: &str) -> Self {
        let mut characters = code.chars();
        let index = characters.next().unwrap_or(' ');
        let worktree = characters.next().unwrap_or(' ');
        if matches!(code, "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU") {
            return Self::Conflict;
        }
        if index == '?' && worktree == '?' {
            return Self::Untracked;
        }
        if index == 'R' || worktree == 'R' {
            return Self::Renamed;
        }
        if index == 'D' || worktree == 'D' {
            return Self::Deleted;
        }
        if index == 'A' || worktree == 'A' {
            return Self::Added;
        }
        Self::Modified
    }
}

/// A branch setting Hide keeps in a repository's own configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchConfigKey {
    /// `branch.<name>.description`: the checkout's purpose.
    Description,
    /// `branch.<name>.issue`: the issue the checkout works on.
    Issue,
}

impl BranchConfigKey {
    fn name(self) -> &'static str {
        match self {
            Self::Description => "description",
            Self::Issue => "issue",
        }
    }
}

/// One git command the core has a node run in a repository on that node's
/// machine. Each is a fixed command line ([`GitCommand::args`]); a request
/// names the branch and value, never the arguments.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum GitCommand {
    /// Every uncommitted change, untracked files included.
    Status,
    /// The absolute path of the repository's shared `.git` folder.
    CommonDir,
    /// The commit a ref names; fails when it names none.
    RefCommit {
        name: String,
    },
    /// How many commits `head` has that `base` does not.
    CountCommits {
        base: String,
        head: String,
    },
    /// Every path in the index, each ended by a NUL.
    ListFiles,
    /// The branch the worktree has checked out.
    CurrentBranch,
    Checkout {
        branch: String,
    },
    /// Answers only when `refs/heads/<branch>` exists.
    HasLocalBranch {
        branch: String,
    },
    /// Fetches `branch` from `origin` into its remote-tracking ref.
    FetchBranch {
        branch: String,
    },
    SetBranchConfig {
        branch: String,
        key: BranchConfigKey,
        value: String,
    },
    /// Removing a setting that is not there succeeds.
    UnsetBranchConfig {
        branch: String,
        key: BranchConfigKey,
    },
}

impl GitCommand {
    /// The arguments after `git --no-optional-locks -C <root>`.
    pub fn args(&self) -> Vec<String> {
        let owned = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect();
        match self {
            Self::Status => owned(&["status", "--porcelain=v1", "--untracked-files=all"]),
            Self::CommonDir => owned(&["rev-parse", "--path-format=absolute", "--git-common-dir"]),
            Self::RefCommit { name } => {
                owned(&["rev-parse", "--verify", &format!("{name}^{{commit}}")])
            }
            Self::CountCommits { base, head } => {
                owned(&["rev-list", "--count", &format!("{base}..{head}"), "--"])
            }
            Self::ListFiles => owned(&["ls-files", "-z"]),
            Self::CurrentBranch => owned(&["symbolic-ref", "--quiet", "--short", "HEAD"]),
            Self::Checkout { branch } => owned(&["checkout", "--no-overwrite-ignore", branch]),
            Self::HasLocalBranch { branch } => owned(&[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ]),
            Self::FetchBranch { branch } => owned(&[
                "fetch",
                "--no-tags",
                "origin",
                &format!("+refs/heads/{branch}:refs/remotes/origin/{branch}"),
            ]),
            Self::SetBranchConfig { branch, key, value } => {
                owned(&["config", &format!("branch.{branch}.{}", key.name()), value])
            }
            Self::UnsetBranchConfig { branch, key } => owned(&[
                "config",
                "--unset-all",
                &format!("branch.{branch}.{}", key.name()),
            ]),
        }
    }
}
