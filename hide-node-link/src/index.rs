//! A checkout's file index as one walk answers it.

use serde::{Deserialize, Serialize};

/// Files one checkout's index holds; past it the index is reported truncated.
pub const INDEX_CAP: usize = 50_000;

/// One walk's answer.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Walked {
    pub paths: Vec<String>,
    pub truncated: bool,
}
