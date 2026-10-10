//! The account's login item that runs the core on the machine a move placed
//! it on (PRD core-host-node-move B9).

use std::path::Path;

/// Installs the login item for the core on `state_dir` and starts it.
pub fn start(_home: &Path, _state_dir: &Path) -> Result<(), String> {
    Err("the login item is not available in this build".to_owned())
}

/// Stops the login item's core and removes the item.
pub fn remove(_home: &Path) -> Result<(), String> {
    Err("the login item is not available in this build".to_owned())
}
