//! What a node's paths are, as the core's catalog reads them: one answer per
//! catalog rebuild, so the core places panes and lists checkouts by comparing
//! names and never reads a folder itself.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use hide_project::git::BranchNote;

/// The most paths one `Call::PathFacts` answers; a longer request is
/// refused, so the core asks in parts or reports the gap.
pub const PATH_FACTS_LIMIT: usize = 4096;

/// The node's answer for the paths the core asked about.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PathFacts {
    /// Each path, by the exact string it was asked as.
    pub paths: BTreeMap<String, PathFact>,
    /// The branch notes of every repository a path is in, by the
    /// repository's main root, read once however many paths it holds.
    pub repositories: BTreeMap<String, Result<BTreeMap<String, BranchNote>, String>>,
}

/// One path as its node reads it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PathFact {
    /// The path with its links resolved as far as it exists, spelled with
    /// `/` between names: the form two paths of one node are compared in.
    pub comparison: String,
    pub exists: bool,
    /// The repository the path is in, read from its `.git` files.
    pub repository: Option<RepositoryPlace>,
}

/// Where a path sits in its repository, all paths in comparison form.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepositoryPlace {
    /// The working tree holding the path.
    pub root: String,
    /// The repository's main working tree, the project's identity.
    pub main_root: String,
    /// The branch checked out in `root`, if any.
    pub branch: Option<String>,
    /// The commit `root`'s HEAD points at, if Git's files name one.
    pub head_oid: Option<String>,
}
