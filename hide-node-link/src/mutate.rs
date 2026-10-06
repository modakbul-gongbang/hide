//! What an Explorer change answers, and the names a change may carry.

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, HostError, HostResult};

/// What a change did, as the caller's result carries it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Changed {}

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
