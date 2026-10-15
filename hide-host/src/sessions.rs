//! A node's agent session files, read for the core's Project Memory and
//! right panel (`hide_node_link::sessions`).

use std::path::Path;
use std::time::UNIX_EPOCH;

use hide_node_link::sessions::SessionStat;

/// The file's size and modification time.
pub fn stat(path: &Path) -> std::io::Result<SessionStat> {
    let metadata = std::fs::metadata(path)?;
    Ok(SessionStat {
        size: metadata.len(),
        modified_unix_ms: metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64),
    })
}
