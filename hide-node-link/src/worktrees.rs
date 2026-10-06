//! The worktree removal shapes a request and its answer carry.

use serde::{Deserialize, Serialize};

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
