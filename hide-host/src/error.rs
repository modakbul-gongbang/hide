use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;

/// Why a host operation was not done. The code is what a caller branches on;
/// the message is the sentence the operator reads, the same whichever machine
/// answered.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HostError {
    pub code: ErrorCode,
    pub message: String,
    /// The revision found on disk when a save was refused as a conflict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_revision: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The request itself is malformed (unknown operation, bad field).
    InvalidRequest,
    /// Empty, absolute, `..`, or otherwise not a path inside the root.
    InvalidPath,
    /// The path resolves outside the opened root (a symlink that leaves it).
    OutsideRoot,
    /// The root's path no longer names the directory that was opened.
    RootReplaced,
    NotFound,
    NotADirectory,
    NotAFile,
    PermissionDenied,
    AlreadyExists,
    /// The file changed since the revision the caller read.
    Conflict,
    TooLarge,
    /// The operation cannot be done safely on this target; nothing changed.
    Unsupported,
    /// The host is at its concurrency or queue limit; nothing was started.
    Busy,
    Cancelled,
    Io,
}

pub type HostResult<T> = Result<T, HostError>;

impl HostError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            actual_revision: None,
        }
    }

    pub fn conflict(actual_revision: Option<String>, message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Conflict,
            message: message.into(),
            actual_revision,
        }
    }

    /// Maps an I/O failure to a code; `what` is the operator-facing sentence.
    pub fn io(error: &io::Error, what: impl Into<String>) -> Self {
        let code = match error.kind() {
            // cap-std reports a path that would leave the directory as a
            // permission failure carrying this text; name it for what it is,
            // before the permission arm can claim it.
            _ if error.to_string().contains("outside of the filesystem") => ErrorCode::OutsideRoot,
            io::ErrorKind::NotFound => ErrorCode::NotFound,
            io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            io::ErrorKind::AlreadyExists => ErrorCode::AlreadyExists,
            io::ErrorKind::NotADirectory => ErrorCode::NotADirectory,
            _ if error.raw_os_error() == Some(libc::ENOTDIR) => ErrorCode::NotADirectory,
            _ => ErrorCode::Io,
        };
        Self::new(code, what)
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for HostError {}
