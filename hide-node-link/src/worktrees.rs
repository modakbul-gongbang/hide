//! A repository's worktree facts, and the removal shapes a request and its
//! answer carry.

use serde::{Deserialize, Serialize};

/// One linked worktree as Git registers it, read with NUL porcelain so no
/// folder name is interpreted.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Registered {
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub locked: bool,
    pub lock_reason: Option<String>,
    pub bare: bool,
    /// Prunable or bare: Git lists it but cannot use it.
    pub unavailable: bool,
}

/// One operator-confirmed worktree deletion, recorded before the host's
/// preflight and reused after Herdr confirms every pane is gone.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfirmedRemoval {
    pub repository_root: String,
    pub checkout_path: String,
    pub expected_head_sha: Option<String>,
    pub expected_branch: Option<String>,
    pub protected_base_branch: Option<String>,
    /// The branch to delete after the folder is gone; `None` keeps it.
    pub delete_branch: Option<String>,
    /// Delete the branch with `git branch -D`: the operator was told it
    /// holds commits the base does not. Otherwise `-d`, which keeps an
    /// unmerged branch. Absent from an older sender, which means `-d`.
    #[serde(default)]
    pub force_delete_branch: bool,
    /// The operator accepted losing the folder's changes and the exact
    /// ignored repositories named below. Git removes with `--force`, while
    /// a lock or an unavailable scan still refuses it.
    /// Absent from an older sender, no discard was accepted.
    #[serde(default)]
    pub discard_changes: bool,
    /// The exact measured names shown in the destructive confirmation.
    #[serde(default)]
    pub expected_ignored_repositories: Vec<String>,
}

/// What a confirmed removal did, as the helper answers it: a stopped
/// removal is an answer too, distinct from a request that got none.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemovalOutcome {
    /// Whether the folder is gone; `false` means the worktree remains.
    pub removed: bool,
    pub message: String,
}

impl From<Result<String, String>> for RemovalOutcome {
    fn from(result: Result<String, String>) -> Self {
        match result {
            Ok(message) => Self {
                removed: true,
                message,
            },
            Err(message) => Self {
                removed: false,
                message,
            },
        }
    }
}

/// A repository's worktrees, as Git reports them. `None` from [`read`] is a
/// folder that is not a repository, which is not a failure.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepositoryWorktrees {
    pub root_path: String,
    pub shared_git_path: Option<String>,
    pub default_branch: Option<String>,
    pub branches: Vec<String>,
    pub base_branch: Option<String>,
    pub base_source: String,
    pub base_branch_fallback: Option<String>,
    pub worktrees: Vec<WorktreeFacts>,
    /// Why this repository has no worktree list. An empty list with no reason
    /// means the repository genuinely has none.
    pub unavailable_reason: Option<String>,
}

/// One worktree's Git facts.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorktreeFacts {
    pub path: String,
    pub branch: Option<String>,
    pub head_sha: Option<String>,
    /// Git lists the worktree but its path is not on disk.
    pub missing: bool,
    pub is_main: bool,
    /// `Some("")` is locked without a reason; `None` is unlocked.
    pub lock_reason: Option<String>,
    /// Measured repository boundaries relative to this checkout.
    pub ignored_repositories: Vec<String>,
    pub ignored_scan_unavailable: Option<String>,
    /// Another listed worktree lies inside this one.
    pub nested: bool,
    pub dirty: bool,
    pub changed_file_count: u32,
    pub base_branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub added_lines: u32,
    pub removed_lines: u32,
    pub merged: Option<bool>,
    pub upstream_state: String,
    pub unpushed: Option<Unpushed>,
    pub behind_upstream: Option<u32>,
    pub created_at_unix_ms: Option<u64>,
    pub last_commit_unix_seconds: Option<u64>,
    pub last_commit_subject: Option<String>,
    pub last_fetch_at_unix_ms: Option<u64>,
    pub measured_at_unix_ms: Option<u64>,
    pub unavailable_reason: Option<String>,
}

/// Commits a branch has that its upstream does not, and the upstream's remote.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Unpushed {
    pub remote: String,
    pub count: u32,
}
