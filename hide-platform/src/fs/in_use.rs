//! Whether a process holds a file in a folder in a way that stops it being
//! removed: a program or library started from it, or a file opened in it
//! without sharing.
//!
//! Windows lets a folder be renamed while a program in it runs: the loader
//! maps the image and keeps no open handle, so neither a rename nor the
//! handle table ([`super::holders`]) sees the program, and only deleting the
//! image fails, after the files beside it are gone. Opening a file for
//! writing is refused while its image is mapped or while another handle holds
//! it without write sharing, so each file is opened that way, written
//! nothing, and closed. Elsewhere a running program never stops its folder
//! being removed, and nothing is in use.

use std::io;
use std::path::{Path, PathBuf};

/// The first file under `folder` that a process holds in use, if any.
#[cfg(windows)]
pub fn first_in_use(folder: &Path) -> io::Result<Option<PathBuf>> {
    use std::os::windows::fs::OpenOptionsExt;
    /// `ERROR_SHARING_VIOLATION`.
    const SHARING_VIOLATION: i32 = 32;
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            if let Some(found) = first_in_use(&path)? {
                return Ok(Some(found));
            }
        } else if kind.is_file() {
            match std::fs::OpenOptions::new()
                .write(true)
                .share_mode(0)
                .open(&path)
            {
                Ok(_) => {}
                Err(error) if error.raw_os_error() == Some(SHARING_VIOLATION) => {
                    return Ok(Some(path));
                }
                Err(error) => return Err(error),
            }
        }
    }
    Ok(None)
}

/// The first file under `folder` that a process holds in use, if any.
#[cfg(not(windows))]
pub fn first_in_use(folder: &Path) -> io::Result<Option<PathBuf>> {
    std::fs::read_dir(folder)?;
    Ok(None)
}
