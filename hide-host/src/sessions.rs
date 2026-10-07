//! A node's agent session files, read for the core's Project Memory and
//! Sessions screen (`hide_node_link::sessions`).

use std::path::Path;
use std::time::UNIX_EPOCH;

use hide_node_link::sessions::{SessionChunk, SessionStat};
use hide_session::{CursorCheckpoint, SessionCursor};

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

/// One bounded read of the complete lines past `checkpoint`.
pub fn chunk(
    path: &Path,
    checkpoint: Option<CursorCheckpoint>,
) -> hide_session::Result<SessionChunk> {
    let mut cursor = checkpoint.map_or_else(SessionCursor::new, SessionCursor::restore);
    let read = cursor.read(path)?;
    Ok(SessionChunk {
        contents: read.contents,
        start_offset: read.start_offset,
        offset: cursor.offset(),
        checkpoint: cursor.checkpoint(),
    })
}
