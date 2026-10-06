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
