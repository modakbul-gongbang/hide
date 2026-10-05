//! Links: the name `current` that always leads to the newest build, and the
//! entries of a folder of links to projects.
//!
//! A link here is a symbolic link on Unix. Windows has two things that act as
//! one, with different costs: a symbolic link to a file needs a privilege or
//! Developer Mode, and a junction to a folder needs neither but only leads to
//! an absolute path on the same machine. So a link to a folder is a junction
//! on Windows, always, and a link to a file is a symbolic link when the
//! account may create one and [`NeedsPrivilege`] when it may not; a caller
//! that has an alternative (copying the file) asks [`needs_privilege`] and
//! takes it.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

/// Making a link to a file needs a privilege this account does not have.
#[derive(Debug)]
pub struct NeedsPrivilege;

impl fmt::Display for NeedsPrivilege {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "making a link to a file needs the privilege to create symbolic links, or Developer Mode, on this system",
        )
    }
}

impl std::error::Error for NeedsPrivilege {}

/// Whether `error` is [`NeedsPrivilege`].
pub fn needs_privilege(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<NeedsPrivilege>())
}

/// Makes `link` lead to `target`. A relative `target` is read from the
/// folder that holds `link`. `link` must not exist.
pub fn create_link(target: &Path, link: &Path) -> io::Result<()> {
    sys::create(target, link)
}

/// Makes `link` lead to `target`, replacing a link that is already there in
/// one step on every system where the system allows it: a link is made
/// beside it and renamed over it, so the name itself is never missing in
/// between and reads as the old target or the new one. Opening a path
/// through the link while it is replaced finds the old target or the new
/// one on macOS and Windows; on Linux that open can, for a moment, find
/// nothing and answer "not found".
pub fn replace_link(target: &Path, link: &Path) -> io::Result<()> {
    let name = link
        .file_name()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let beside = link
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(super::temporary_name(name, "hide-link"));
    create_link(target, &beside)?;
    sys::rename_over(&beside, link).inspect_err(|_| {
        let _ = sys::remove(&beside);
    })
}

/// Removes the link `link` itself, never what it leads to.
pub fn remove_link(link: &Path) -> io::Result<()> {
    sys::remove(link)
}

/// Whether `link` is itself a link, and leads to `target`. A relative
/// `target` is read from the folder that holds `link`.
pub fn is_link_to(link: &Path, target: &Path) -> bool {
    fs::symlink_metadata(link).is_ok_and(|metadata| metadata.file_type().is_symlink())
        && sys::leads_to(link, target)
}

#[cfg(unix)]
mod sys {
    use std::fs;
    use std::io;
    use std::path::Path;

    pub(super) fn create(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    pub(super) fn rename_over(from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    pub(super) fn remove(link: &Path) -> io::Result<()> {
        fs::remove_file(link)
    }

    pub(super) fn leads_to(link: &Path, target: &Path) -> bool {
        fs::read_link(link).is_ok_and(|destination| destination == target)
    }
}

#[cfg(windows)]
mod sys {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Foundation::ERROR_PRIVILEGE_NOT_HELD;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateSymbolicLinkW, SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE,
    };

    use super::NeedsPrivilege;
    use crate::fs::identity::canonical;
    use crate::fs::wide;

    /// `target` read from the folder that holds `link`.
    fn absolute(target: &Path, link: &Path) -> PathBuf {
        link.parent().unwrap_or_else(|| Path::new("")).join(target)
    }

    pub(super) fn create(target: &Path, link: &Path) -> io::Result<()> {
        let wanted = absolute(target, link);
        if fs::metadata(&wanted).is_ok_and(|metadata| metadata.is_dir()) {
            return junction::create(wanted, link);
        }
        let (link_name, target_name) = (wide(link)?, wide(target)?);
        // SAFETY: both names are NUL-terminated and outlive the call.
        let made = unsafe {
            CreateSymbolicLinkW(
                link_name.as_ptr(),
                target_name.as_ptr(),
                SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE,
            )
        };
        if made {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD as i32) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                NeedsPrivilege,
            ));
        }
        Err(error)
    }

    pub(super) fn rename_over(from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    pub(super) fn remove(link: &Path) -> io::Result<()> {
        // A link to a folder (a junction, or a symbolic link made for one) is
        // removed as a folder, which removes the link and leaves the folder it
        // leads to.
        let is_link = fs::symlink_metadata(link).is_ok_and(|meta| meta.file_type().is_symlink());
        match fs::remove_file(link) {
            Err(error) if is_link => fs::remove_dir(link).map_err(|_| error),
            result => result,
        }
    }

    pub(super) fn leads_to(link: &Path, target: &Path) -> bool {
        let Ok(destination) = fs::read_link(link) else {
            return false;
        };
        if destination == target {
            return true;
        }
        // A junction leads to the absolute form of what it was made with.
        match (
            canonical(&absolute(&destination, link)),
            canonical(&absolute(target, link)),
        ) {
            (Ok(found), Ok(wanted)) => found == wanted,
            _ => false,
        }
    }
}
