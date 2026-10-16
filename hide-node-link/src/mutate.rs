//! What an Explorer change answers, and the names a change may carry.

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, HostError, HostResult};

/// What a change did, as the caller's result carries it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Changed {}

/// What a copy made: the name its top-level item took in the destination
/// folder, which the host chose because the wanted one may have been taken
/// (`name copy.ext`, `name copy 2.ext`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Copied {
    pub name: String,
}

/// What a dropped set of files made: the name each top-level item took, in
/// the order the request listed them.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Imported {
    pub names: Vec<String>,
}

/// One file or folder of a drop, below the folder it lands in.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImportEntry {
    /// The wire spelling of a path below the destination: `/` between names.
    pub path: String,
    pub directory: bool,
    /// The file's bytes, base64 encoded for the JSON line; empty for a folder.
    #[serde(default)]
    pub data: String,
}

/// Items and bytes one copy may hold. A copy is refused whole past either,
/// before the first byte is written.
pub const COPY_MAX_ITEMS: usize = 5_000;
pub const COPY_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// Items and bytes one drop from the file manager may hold, and the bytes of
/// one file in it: the page uploads them to hided's staging, which holds
/// 20 MiB per file and 40 MiB per batch (`hided/src/attachments.rs`).
pub const IMPORT_MAX_ITEMS: usize = 500;
pub const IMPORT_MAX_BYTES: u64 = 40 * 1024 * 1024;
pub const IMPORT_MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;

/// A name a creation or a rename may carry: one component, and not one of the
/// shapes that would name a different path.
pub fn valid_name(name: &str) -> HostResult<&str> {
    let refuse = |message: String| Err(HostError::new(ErrorCode::InvalidPath, message));
    if name.is_empty() {
        return refuse("A name is required".to_owned());
    }
    if name.contains('/') {
        return refuse("A name cannot contain /".to_owned());
    }
    if name.contains('\0') {
        return refuse("A name cannot contain NUL".to_owned());
    }
    if name == "." || name == ".." {
        return refuse(format!("{name} is not a valid name"));
    }
    // A name this system would read as something else (a `\` on Windows,
    // a device name, a trailing dot) is refused rather than created as it.
    if let Err(error) = hide_platform::path::RelPath::root()
        .join(name)
        .and_then(|path| path.to_native())
    {
        return refuse(format!("{name} is not a valid name here: {error}"));
    }
    Ok(name)
}
