//! What a node's kernel says about a process.

use serde::{Deserialize, Serialize};

/// One pid as the node read it. The start time names the process, so a pid
/// the system hands to another process later is not mistaken for it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ProcessStart {
    Running {
        started: u64,
    },
    /// No process has the pid.
    Gone,
    /// The process could not be read; it is not claimed to be gone.
    Unreadable {
        reason: String,
    },
}
