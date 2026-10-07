//! A device's panes asking the core for a Workspace credential and running
//! `hide` commands with it, over the node's link (PRD core-host-node B18,
//! B19). The node proves each caller with its own kernel and sends the proof
//! up; the core issues the credential and answers down. A command's bytes
//! travel the same link as a stream, so a device pane needs no other
//! connection to the core.

use serde::{Deserialize, Serialize};

/// Streams one link carries at once.
pub const MAX_STREAMS: usize = 8;
/// The most bytes of a stream one message carries.
pub const MAX_CHUNK: usize = 48 * 1024;
/// Chunks the core holds for one stream before the stream is ended.
pub const MAX_PENDING_CHUNKS: usize = 64;

/// Which shell a pane runs, as its Herdr terminal and the shell's pid and
/// start: the identity a bootstrap checks a caller against.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct PaneIdentity {
    pub terminal_id: String,
    pub shell_pid: i32,
    pub shell_started: u64,
}

/// What a node tells the core without being asked: one JSON line with an
/// `event` field and no `id`.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum NodeEvent {
    /// A process in `pane_id` asked for a credential, and the node's kernel
    /// says it descends from that pane's shell. `request` pairs the core's
    /// [`ProofAnswer`].
    PaneProof {
        request: u64,
        pane_id: String,
        #[serde(flatten)]
        identity: PaneIdentity,
        nonce: String,
        one_shot: bool,
    },
    /// A credential the node handed out is no longer held: its reference
    /// expired, was removed, or its holder ended.
    Revoke { token: String },
    /// A `hide` command opened a stream.
    StreamOpen { stream: u64 },
    /// Base64 bytes the command sent.
    StreamData { stream: u64, data: String },
    /// The command's side of the stream ended.
    StreamClosed { stream: u64 },
    /// The node turned a caller away on its own, before the core was asked
    /// or at one of its caps, with the reason the caller read; the core
    /// records it with the node, since a device's stderr reaches no log.
    Refused {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane_id: Option<String>,
        reason: String,
    },
}

/// The core's answer to a [`NodeEvent::PaneProof`]: the credential, or why
/// there is none in the reason codes a caller already acts on.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ProofAnswer {
    Issued {
        token: String,
        /// The token was made for this proof; one already held by the pane
        /// is not, and a failed delivery must not revoke it.
        issued_new: bool,
    },
    Refused {
        reason: String,
    },
}

/// What `Call::PanesStart` answers: where the node's bootstrap socket is.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct PanesStarted {
    pub socket: String,
}
