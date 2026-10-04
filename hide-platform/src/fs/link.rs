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

/// Publication succeeded, but the displaced link could not be removed.
/// The caller must not interpret this error as an unchanged destination.
#[derive(Debug)]
pub struct PublishedCleanupFailure {
    pub displaced: std::path::PathBuf,
    pub source: io::Error,
}

impl fmt::Display for PublishedCleanupFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "link published but displaced link cleanup failed: {}",
            self.source
        )
    }
}

impl std::error::Error for PublishedCleanupFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

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
/// beside it and renamed over it, so the name never leads nowhere in
/// between.
pub fn replace_link(target: &Path, link: &Path) -> io::Result<()> {
    let name = link
        .file_name()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let beside = link
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(super::temporary_name(name, "hide-link"));
    create_link(target, &beside)?;
    sys::rename_over(&beside, link).inspect_err(|error| {
        // After publication the displaced entry belongs to the caller's
        // recovery, not a second best-effort delete hiding the failure.
        if !error
            .get_ref()
            .is_some_and(|inner| inner.is::<PublishedCleanupFailure>())
        {
            let _ = sys::remove(&beside);
        }
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

    #[cfg(not(target_os = "macos"))]
    pub(super) fn rename_over(from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn rename_over(from: &Path, to: &Path) -> io::Result<()> {
        // Native directory-symlink open/rename controls expose EINVAL with
        // ordinary rename-over; exchanged publication passes those controls.
        // This is a boundary observation, not an inferred kernel cause.
        // Exchange keeps both link identities through publication, then removes our displaced
        // link. Reuse the platform's existing atomic exchange implementation.
        // Swap accepts mixed file/directory types, unlike rename-over. Only
        // existing symlinks use it; other kinds keep ordinary rename's refusal
        // semantics, including an unchanged directory destination.
        match fs::symlink_metadata(to) {
            Ok(metadata) if metadata.file_type().is_symlink() => {}
            Ok(_) => return fs::rename(from, to),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return fs::rename(from, to),
            Err(error) => return Err(error),
        }
        let parent = to.parent().unwrap_or_else(|| Path::new("."));
        let folder = crate::fs::open_dir(parent)?;
        let from_name = from
            .file_name()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let to_name = to
            .file_name()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        match crate::fs::atomic::exchange(&folder, from_name, to_name) {
            Ok(()) => fs::remove_file(from).map_err(|source| {
                io::Error::new(
                    source.kind(),
                    super::PublishedCleanupFailure {
                        displaced: from.to_owned(),
                        source,
                    },
                )
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => fs::rename(from, to),
            Err(error) => Err(error),
        }
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
