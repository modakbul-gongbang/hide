//! The shapes of a node's agent sessions, which Project Memory and the
//! Sessions screen read through the core (PRD core-host-node D-03).

use hide_session::CursorCheckpoint;
use serde::{Deserialize, Serialize};

/// A session file's size and modification time, read when asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStat {
    pub size: u64,
    /// `None` where the system keeps no modification time.
    pub modified_unix_ms: Option<u64>,
}

/// The complete lines past a cursor (`hide_session::SessionCursor::read`):
/// `contents` begins at `start_offset`, `offset` is where the read stopped,
/// and `checkpoint` is what the next read starts from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionChunk {
    pub contents: String,
    pub start_offset: u64,
    pub offset: u64,
    pub checkpoint: CursorCheckpoint,
}
