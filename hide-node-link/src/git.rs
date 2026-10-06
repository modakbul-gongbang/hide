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
