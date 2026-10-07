//! One folder of a checkout as the Explorer lists it.

use serde::{Deserialize, Serialize};

/// Children a listing carries at most; a folder with more is answered as
/// truncated, never grown (PRD S5.5 B6).
pub const LIST_CAP: usize = 500;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub is_directory: bool,
    /// The entry's own inode (a link's, not its target's): the identity a
    /// trash of this row confirms (`mutate::trash`).
    pub inode: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// More than `LIST_CAP` children existed; the rest were not read.
    pub truncated: bool,
}
