//! A session file's change token, compared by readers to know whether a
//! file moved since they last read it. An observation, never read
//! authority: a reader still confirms what it reads.

use std::fs;
use std::path::Path;

/// The current stamp of the file at `path`, `None` when it is gone or
/// unreadable.
pub fn stamp_at(path: &Path) -> Option<String> {
    if path.file_name().is_some_and(|name| name == "store.db") {
        crate::cursor::source_stamp(path)
    } else {
        metadata_stamp(&fs::metadata(path).ok()?)
    }
}

fn metadata_stamp(m: &fs::Metadata) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(format!(
            "{}:{}:{}:{}:{}",
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec()
        ))
    }
    #[cfg(not(unix))]
    {
        Some(format!("{}:{:?}", m.len(), m.modified().ok()?))
    }
}
