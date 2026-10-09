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

/// How the terminal a pane's shell controls takes typed input
/// (`hide_platform::process::LineInput`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum LineInput {
    /// Each key reaches the program as it arrives: the shell's line editor
    /// is reading.
    Keys,
    /// The kernel holds an unfinished line and drops every byte past
    /// `limit` of it, Enter included.
    Lines { limit: u32 },
    /// A console with no line discipline, which cuts no line.
    Console,
}
