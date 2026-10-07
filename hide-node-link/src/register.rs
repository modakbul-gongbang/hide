//! The project a folder would be registered as.

use serde::{Deserialize, Serialize};

/// The project a registrable folder belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registrable {
    /// The project's root: the main worktree of its repository, or the
    /// folder itself.
    pub root: String,
    pub is_git: bool,
}
