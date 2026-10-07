//! Files the operator picked to attach to a terminal, as the node that holds
//! them reads them: the limits both sides keep, and each file's bytes.

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const MAX_FILES: usize = 8;
pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_REQUEST_BYTES: u64 = 40 * 1024 * 1024;

/// How long a committed file is safe from eviction. The pasted token is the
/// staged file's path, so the core waits no longer than this for a terminal
/// to paste it into.
pub const COMMIT_GRACE: Duration = Duration::from_secs(60);

/// One picked file, read whole and unchanged while it was read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReadFile {
    pub path: String,
    pub name: String,
    /// The bytes, base64 encoded for the JSON line.
    pub data: String,
}

impl ReadFile {
    pub fn new(path: String, name: String, bytes: &[u8]) -> Self {
        Self {
            path,
            name,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }

    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        base64::engine::general_purpose::STANDARD
            .decode(&self.data)
            .map_err(|error| format!("A selected file's bytes arrived damaged: {error}"))
    }
}
