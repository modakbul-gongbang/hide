//! What a node's kernel says about a process.

use serde::{Deserialize, Serialize};

/// One pid as the node read it. The start time identifies the process, so a
/// pid the system hands to another process later is not mistaken for it, and
/// the name is the program it runs, so a failure can say which one to end.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ProcessStart {
    Running {
        started: u64,
        name: String,
    },
    /// No process has the pid.
    Gone,
    /// The process could not be read; it is not claimed to be gone.
    Unreadable {
        reason: String,
    },
}
