//! The pinned Herdr a device gets with its helper, and what its node did
//! about the device's Herdr server (PRD device-herdr-update).
//!
//! A package that carries Herdr for the device's system puts it in
//! `<root>/herdr/<id>/herdr`, beside the build folders rather than in one,
//! so a build that changes only the helper finds it already there; each
//! build folder names its copy in `herdr.pin`. Before the kit applies, the
//! node points `<root>/herdr/current` at its build's copy, so Hide's own
//! uses of Herdr on the device (the status probe, the kit, the terminal
//! attaches) name one path that outlives a build.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The folder under a helper root that holds the pinned Herdr copies.
pub const HERDR_DIR: &str = "herdr";
/// The file in a build folder that names its pinned Herdr copy's folder.
pub const HERDR_PIN: &str = "herdr.pin";
/// The program's name in a copy's folder.
pub const HERDR_PROGRAM: &str = "herdr";
/// The record of the device's failed handoffs (D-13), in [`HERDR_DIR`].
pub const HANDOFF_RECORD: &str = "handoff.json";

/// The folder holding the pinned copies under `root`.
pub fn herdr_dir(root: &Path) -> PathBuf {
    root.join(HERDR_DIR)
}

/// The copy the running build pins, through the link its node points before
/// the kit applies: the path Hide's own Herdr calls on a device use.
pub fn pinned_herdr(root: &Path) -> PathBuf {
    herdr_dir(root).join(crate::CURRENT).join(HERDR_PROGRAM)
}

/// The pinned copy for the helper running from `executable`
/// (`<root>/<build>/hided`), when the build brought one; `None` for any
/// other program, such as a development build or the desktop package.
pub fn pinned_for_helper(executable: &Path) -> Option<PathBuf> {
    let build = executable.parent()?;
    build
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| crate::is_build_name(name))?;
    let pinned = pinned_herdr(build.parent()?);
    pinned.is_file().then_some(pinned)
}

/// What the device's node found and did about the Herdr server on the
/// device's registered socket after a kit pass (PRD device-herdr-update D-07).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum HerdrUpdate {
    /// Nothing answers on the socket.
    NoServer,
    /// The server's Herdr is already the pinned version, whatever file it
    /// runs.
    Current { version: String },
    /// The server's Herdr is newer than the pin, or a release the pin does
    /// not order against (a preview), so it is left as it is.
    Kept {
        server_version: String,
        version: String,
    },
    /// The server's panes now belong to a server running the pinned copy.
    HandedOff {
        server_version: String,
        version: String,
        from_pid: u32,
        to_pid: u32,
        elapsed_ms: u64,
    },
    /// The server runs an older Herdr than the pin and stays on it.
    Behind {
        server_version: String,
        version: String,
        cause: BehindCause,
        /// Why, for the log; the screen reads only `cause`.
        reason: String,
    },
    /// The node could not tell which Herdr the server runs, or could not run
    /// the pinned copy; nothing was changed.
    Failed { stage: String, reason: String },
}

/// Why a server older than the pin stays on its Herdr, which decides what
/// the operator can do about it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehindCause {
    /// Its Herdr cannot hand its panes over: only updating that machine's
    /// Herdr changes it.
    CannotHandOff,
    /// The handoff was refused, timed out or did not take, now or on an
    /// earlier connection to the same server: a restarted server is tried
    /// again.
    HandoffFailed,
}

impl HerdrUpdate {
    /// The one log record of this outcome (B12), with the device it is for.
    pub fn record(&self, device_id: &str) -> serde_json::Value {
        let mut record = serde_json::json!(self);
        record["component"] = "kit".into();
        record["kind"] = "herdr.handoff".into();
        record["device_id"] = device_id.into();
        record
    }
}
