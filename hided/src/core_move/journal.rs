//! `core-move.json`: the driver's record of a move, written before every
//! step that cannot be undone (PRD core-host-node-move B5, B6, amendment 1).
//!
//! A start that finds one past `Stopping` and not finished never starts a
//! core on its own: before `AttachSent` the move is rolled back (the other
//! machine's pending core is stopped first), from `AttachSent` the other
//! machine's handover decides, and from `Committed` the move only goes
//! forward.

use std::path::{Path, PathBuf};

use herdr_core::node_migration::{IdTable, OwnerChange};
use hide_platform::fs::Access;
use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;
const RECORD_CAP: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Journal {
    pub version: u32,
    pub intent: String,
    pub direction: Direction,
    /// The other machine, as this one reaches it.
    pub peer: Peer,
    /// The copy's owner change and the ids it maps.
    pub change: OwnerChange,
    pub ids: IdTable,
    pub phase: Phase,
    /// When the other machine's core was started, on this machine's wall
    /// clock: a pending core that never got its link gives its copy back
    /// after its lease, which a driver that cannot reach it waits out.
    #[serde(default)]
    pub target_started_unix_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// This machine's core goes to the peer.
    Forward,
    /// The peer's core comes back to this machine.
    Back,
}

/// The other machine of a move.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Peer {
    /// Its registration id in the core before the move.
    pub device: String,
    /// The SSH alias this machine reaches it by.
    pub alias: String,
    /// Its node id.
    pub node: String,
    /// The `hided` there that runs the move's steps and, after a forward
    /// move, the attach role.
    pub program: String,
    /// Its state folder, as it reported it.
    pub state_dir: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "phase")]
pub enum Phase {
    /// The core is stopping or stopped; nothing has left this machine.
    Stopping,
    /// The copy is on the peer and verified.
    Sent,
    /// The copy is placed on the peer under a pending handover.
    Placed,
    /// The peer's pending core started.
    TargetStarted,
    /// The link carrying the intent may have reached the peer: from here
    /// only the peer's handover says whether the move committed.
    AttachSent,
    /// The peer's core took the link: the move only goes forward.
    Committed,
    /// This machine's brain state is set aside and the peer's records are
    /// cleared.
    Done,
    /// The move was undone; this machine's core runs on its folder again.
    RolledBack {
        failed: MoveStep,
        cause: MoveFailure,
    },
}

impl Phase {
    /// Whether a start finding this phase must not start a core of its own
    /// before the move is resolved.
    pub fn holds_the_core(&self) -> bool {
        !matches!(self, Self::Done | Self::RolledBack { .. })
    }

    /// The step a move interrupted in this phase was at.
    pub fn step(&self) -> MoveStep {
        match self {
            Self::Stopping => MoveStep::StopCore,
            Self::Sent | Self::Placed => MoveStep::Copy,
            Self::TargetStarted => MoveStep::StartTarget,
            Self::AttachSent | Self::Committed | Self::Done => MoveStep::Reattach,
            Self::RolledBack { failed, .. } => *failed,
        }
    }
}

/// The five steps the move's window names.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveStep {
    Check,
    StopCore,
    Copy,
    StartTarget,
    Reattach,
}

/// Why a move failed, typed so the window and the log say which step and
/// what to do without reading a message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum MoveFailure {
    /// The peer could not be reached over SSH.
    Unreachable { reason: String },
    /// The core did not stop.
    StopUnconfirmed { reason: String },
    /// The copy could not be made or its owner changed: the named file.
    Staging { file: String, reason: String },
    /// The copy did not reach the peer whole.
    Copy { reason: String },
    /// The peer's copy still differs after it was sent again.
    Digest { files: Vec<String> },
    /// The peer's build cannot load the copy: the named file.
    Load { file: String, reason: String },
    /// The peer refused a step.
    Refused { step: String, reason: String },
    /// The peer's core exited at its start or did not take links in time.
    NotStarted { reason: String },
    /// The peer's core refused this machine's first link.
    LinkRefused { reason: String },
    /// Something on this machine failed.
    Local { reason: String },
}

pub fn path(state_dir: &Path) -> PathBuf {
    hide_kit::layout::core_move_journal(state_dir)
}

/// The journal `state_dir` holds; `None` when no move was ever driven here.
pub fn read(state_dir: &Path) -> Result<Option<Journal>, String> {
    let path = path(state_dir);
    let bytes = match super::read_private(&path, RECORD_CAP) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let journal: Journal = serde_json::from_slice(&bytes)
        .map_err(|error| format!("{} is not a move this build reads: {error}", path.display()))?;
    if journal.version != VERSION {
        return Err(format!(
            "{} has version {}, which this build does not read",
            path.display(),
            journal.version
        ));
    }
    Ok(Some(journal))
}

/// Records `journal`, flushed before this returns.
pub fn write(state_dir: &Path, journal: &Journal) -> Result<(), String> {
    let path = path(state_dir);
    let bytes = serde_json::to_vec_pretty(journal).map_err(|error| error.to_string())?;
    hide_platform::fs::atomic::write_file_durable(&path, &bytes, Access::Private)
        .map(|_| ())
        .map_err(|error| format!("{}: {error}", path.display()))
}

impl Journal {
    pub fn new(
        intent: String,
        direction: Direction,
        peer: Peer,
        change: OwnerChange,
        ids: IdTable,
    ) -> Self {
        Self {
            version: VERSION,
            intent,
            direction,
            peer,
            change,
            ids,
            phase: Phase::Stopping,
            target_started_unix_ms: None,
        }
    }
}
