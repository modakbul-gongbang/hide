//! One range of a file's bytes.

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, HostError, HostResult};

/// Which file a range came from: the same device, inode, size and
/// modification time across ranges is the same unchanged file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileStamp {
    pub device: u64,
    pub inode: u64,
    pub modified_ns: i128,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Range {
    /// The file's whole size when this range was read.
    pub total: u64,
    pub offset: u64,
    /// The bytes, base64 encoded for the JSON line.
    pub data: String,
    pub file: FileStamp,
}

impl Range {
    pub fn bytes(&self) -> HostResult<Vec<u8>> {
        base64::engine::general_purpose::STANDARD
            .decode(&self.data)
            .map_err(|error| {
                HostError::new(
                    ErrorCode::Io,
                    format!("The file's bytes arrived damaged: {error}"),
                )
            })
    }
}
