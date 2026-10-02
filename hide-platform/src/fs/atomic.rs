//! Replacing a file, and moving one, so that no reader and no crash sees a
//! half-done change.
//!
//! The path of a file that is being replaced names either the old file or the
//! new one at every moment and never a truncated one, on every system.
//! What differs is what else the system gives: macOS and Linux can exchange
//! two names in one step and can rename without replacing, which a save that
//! checks what it displaced is built on; Windows has `ReplaceFileW`, which
//! replaces and keeps the old file under another name, and `MoveFileExW`,
//! which refuses to replace. [`EXCHANGE_IS_ATOMIC`] says which of the two a
//! build has.

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use super::identity::{self, FileId};
use super::permissions::Permissions;
use super::{Access, Handle};

/// Whether [`exchange`] swaps the two names in one step. Where it does not
/// (Windows), the name that is replaced still never goes missing or shows a
/// partial file, but the name that receives the old file is empty for a moment
/// in between, and a crash in that moment leaves the old file under a
/// `.hide-swap-` name beside it instead of lost.
pub const EXCHANGE_IS_ATOMIC: bool = cfg!(any(target_os = "macos", target_os = "linux"));

/// Writes `contents` to `path` whole or not at all: the bytes go to a file
/// beside it, are flushed, and replace `path` in one step. A failure at any
/// point leaves what was at `path` as it was and the temporary file removed.
/// Returns the identity of the file now at `path`.
///
/// `path` itself is replaced, so a link there is replaced by a file; a caller
/// that means to write through a link resolves it first with
/// [`identity::canonical`].
pub fn write_file(path: &Path, contents: &[u8], access: Access) -> io::Result<FileId> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let folder = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let kept = match access {
        Access::KeepOrPrivate => match fs::File::open(path) {
            Ok(existing) => Some(Permissions::of(&existing)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        },
        Access::Private | Access::PrivateExecutable => None,
    };
    let mut last = io::Error::from(io::ErrorKind::AlreadyExists);
    for _ in 0..8 {
        let temporary = folder.join(super::temporary_name(name, "hide"));
        let mut file = match sys::create_temporary(&temporary, access, kept.is_some()) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                last = error;
                continue;
            }
            Err(error) => return Err(error),
        };
        let written = (|| {
            file.write_all(contents)?;
            if let Some(kept) = &kept {
                kept.apply(&file)?;
            }
            file.sync_all()?;
            let id = identity::file_id_of(&file)?;
            drop(file);
            sys::replace(&temporary, path, kept.is_some())?;
            Ok(id)
        })();
        return match written {
            Ok(id) => {
                sync_path_parent(folder);
                Ok(id)
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                Err(error)
            }
        };
    }
    Err(last)
}

/// Renames the file `from` over `to`, replacing it in one step. Returns the
/// identity of the file now at `to`, which is the one `from` had.
pub fn replace_file(from: &Path, to: &Path) -> io::Result<FileId> {
    // The entry itself: a link moves as a link, and its id is its own.
    let id = identity::file_id_nofollow(from)?;
    fs::rename(from, to)?;
    if let Some(folder) = to.parent().filter(|folder| !folder.as_os_str().is_empty()) {
        sync_path_parent(folder);
    }
    Ok(id)
}

/// Makes the changes to the open folder `dir`'s entries durable: a file
/// that was synced and renamed is not yet on disk until its folder is.
/// Windows keeps no such separate step, and answers `Ok`.
pub fn sync_dir(dir: &impl Handle) -> io::Result<()> {
    sys::sync_dir(dir)
}

fn sync_path_parent(folder: &Path) {
    if let Ok(folder) = fs::File::open(folder) {
        let _ = sys::sync_dir(&folder);
    }
}

/// Swaps two names in the open folder `dir`: afterwards `left` holds what
/// `right` held and the other way round. Both must exist. A filesystem
/// that cannot is `ErrorKind::Unsupported`, and nothing changed.
///
/// On Windows the swap is a replace followed by a rename and is not one step;
/// see [`EXCHANGE_IS_ATOMIC`].
pub fn exchange(dir: &impl Handle, left: &OsStr, right: &OsStr) -> io::Result<()> {
    sys::exchange(dir, left, right)
}

/// [`rename_no_replace`] for a caller that has two paths and no open folders:
/// a finished clone moved to its name, a staged folder moved to `~/hide`.
pub fn rename_no_replace_path(from: &Path, to: &Path) -> io::Result<()> {
    sys::rename_no_replace_path(from, to)
}

/// Renames `from` in `from_dir` to `to` in `to_dir`, and refuses when `to`
/// exists, even when it appears between the caller's look and this call
/// (`ErrorKind::AlreadyExists`). `ErrorKind::CrossesDevices` when the two
/// folders are on different volumes.
pub fn rename_no_replace(
    from_dir: &impl Handle,
    from: &OsStr,
    to_dir: &impl Handle,
    to: &OsStr,
) -> io::Result<()> {
    sys::rename_no_replace(from_dir, from, to_dir, to)
}

#[cfg(unix)]
mod sys {
    use std::ffi::{CString, OsStr};
    use std::fs;
    use std::io;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::Path;

    use super::{Access, Handle};

    pub(super) fn create_temporary(
        path: &Path,
        access: Access,
        _inherit: bool,
    ) -> io::Result<fs::File> {
        let mode = match access {
            Access::PrivateExecutable => 0o700,
            Access::Private | Access::KeepOrPrivate => 0o600,
        };
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(path)
    }

    pub(super) fn replace(from: &Path, to: &Path, _keep_acl: bool) -> io::Result<()> {
        fs::rename(from, to)
    }

    pub(super) fn sync_dir(dir: &impl Handle) -> io::Result<()> {
        // A folder handle that cannot be flushed (cap-std's `O_PATH` one) is
        // reopened readable first.
        let file = crate::fs::reopen_dir_for_io(dir)?;
        // The plain flush: `sync_all` is `F_FULLFSYNC` on macOS, which on a
        // folder costs milliseconds more than the `fsync` a save always paid.
        // SAFETY: the descriptor belongs to `file`, which outlives the call.
        if unsafe { libc::fsync(std::os::fd::AsRawFd::as_raw_fd(&file)) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn name(value: &OsStr) -> io::Result<CString> {
        CString::new(value.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a name holds a NUL"))
    }

    #[cfg(target_os = "macos")]
    pub(super) fn exchange(dir: &impl Handle, left: &OsStr, right: &OsStr) -> io::Result<()> {
        let (left, right) = (name(left)?, name(right)?);
        let fd = dir.as_fd().as_raw_fd();
        // SAFETY: both names are NUL-terminated and outlive the call, and the
        // descriptor is an open folder borrowed for it.
        let result =
            unsafe { libc::renameatx_np(fd, left.as_ptr(), fd, right.as_ptr(), libc::RENAME_SWAP) };
        finish_exchange(result)
    }

    #[cfg(target_os = "linux")]
    pub(super) fn exchange(dir: &impl Handle, left: &OsStr, right: &OsStr) -> io::Result<()> {
        let (left, right) = (name(left)?, name(right)?);
        let fd = dir.as_fd().as_raw_fd();
        // SAFETY: as on macOS.
        let result = unsafe {
            libc::renameat2(fd, left.as_ptr(), fd, right.as_ptr(), libc::RENAME_EXCHANGE)
        };
        finish_exchange(result)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub(super) fn exchange(_: &impl Handle, _: &OsStr, _: &OsStr) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    /// The codes a filesystem without an atomic exchange answers with, said
    /// as one kind so a caller does not read errno.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn finish_exchange(result: i32) -> io::Result<()> {
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        // `ENOTSUP` and `EOPNOTSUPP` are one number on Linux and two on macOS.
        let unsupported = error.raw_os_error().is_some_and(|code| {
            [libc::ENOTSUP, libc::EOPNOTSUPP, libc::EINVAL, libc::ENOSYS].contains(&code)
        });
        Err(if unsupported {
            io::Error::from(io::ErrorKind::Unsupported)
        } else {
            error
        })
    }

    #[cfg(target_os = "macos")]
    pub(super) fn rename_no_replace(
        from_dir: &impl Handle,
        from: &OsStr,
        to_dir: &impl Handle,
        to: &OsStr,
    ) -> io::Result<()> {
        let (from, to) = (name(from)?, name(to)?);
        // SAFETY: both names are NUL-terminated and outlive the call, and both
        // descriptors are open folders borrowed for it.
        let result = unsafe {
            libc::renameatx_np(
                from_dir.as_fd().as_raw_fd(),
                from.as_ptr(),
                to_dir.as_fd().as_raw_fd(),
                to.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        finish_rename(result)
    }

    #[cfg(target_os = "linux")]
    pub(super) fn rename_no_replace(
        from_dir: &impl Handle,
        from: &OsStr,
        to_dir: &impl Handle,
        to: &OsStr,
    ) -> io::Result<()> {
        let (from, to) = (name(from)?, name(to)?);
        // SAFETY: as on macOS.
        let result = unsafe {
            libc::renameat2(
                from_dir.as_fd().as_raw_fd(),
                from.as_ptr(),
                to_dir.as_fd().as_raw_fd(),
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        finish_rename(result)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub(super) fn rename_no_replace(
        _: &impl Handle,
        _: &OsStr,
        _: &impl Handle,
        _: &OsStr,
    ) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    #[cfg(target_os = "macos")]
    pub(super) fn rename_no_replace_path(from: &Path, to: &Path) -> io::Result<()> {
        let (from, to) = (name(from.as_os_str())?, name(to.as_os_str())?);
        // SAFETY: both are NUL-terminated paths that outlive the call.
        finish_rename(unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) })
    }

    #[cfg(target_os = "linux")]
    pub(super) fn rename_no_replace_path(from: &Path, to: &Path) -> io::Result<()> {
        let (from, to) = (name(from.as_os_str())?, name(to.as_os_str())?);
        // SAFETY: both are NUL-terminated paths that outlive the call.
        finish_rename(unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub(super) fn rename_no_replace_path(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn finish_rename(result: i32) -> io::Result<()> {
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::OsStr;
    use std::fs;
    use std::io;
    use std::path::Path;
    use std::ptr::null;

    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, ReplaceFileW};

    use super::{Access, Handle};
    use crate::fs::{path_of, wide};

    pub(super) fn create_temporary(
        path: &Path,
        access: Access,
        inherit: bool,
    ) -> io::Result<fs::File> {
        // A temporary that will replace a file through `ReplaceFileW` takes
        // that file's access list in the replacement, so it needs none of its
        // own; any other is private from its first moment.
        let _ = access;
        if inherit {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
        } else {
            crate::fs::private::create_new_file(path)
        }
    }

    pub(super) fn replace(from: &Path, to: &Path, keep_acl: bool) -> io::Result<()> {
        if keep_acl {
            return replace_file_w(to, from, None);
        }
        fs::rename(from, to)
    }

    pub(super) fn sync_dir(_: &impl Handle) -> io::Result<()> {
        Ok(())
    }

    /// `replacement` takes the name `replaced` had, with its attributes and
    /// access list, and `replaced`'s old file goes to `backup` when given.
    fn replace_file_w(
        replaced: &Path,
        replacement: &Path,
        backup: Option<&Path>,
    ) -> io::Result<()> {
        let (replaced, replacement) = (wide(replaced)?, wide(replacement)?);
        let backup = backup.map(wide).transpose()?;
        // SAFETY: every name is NUL-terminated and outlives the call; the two
        // reserved arguments are null as the call requires.
        let done = unsafe {
            ReplaceFileW(
                replaced.as_ptr(),
                replacement.as_ptr(),
                backup.as_ref().map_or(null(), |name| name.as_ptr()),
                0,
                null(),
                null(),
            )
        };
        if done == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// `ReplaceFileW` puts `left` under `right`'s name and `right`'s old file
    /// under a parked name, and a rename then moves that file to `left`'s.
    pub(super) fn exchange(dir: &impl Handle, left: &OsStr, right: &OsStr) -> io::Result<()> {
        let folder = path_of(dir)?;
        let parked = folder.join(crate::fs::temporary_name(right, "hide-swap"));
        replace_file_w(&folder.join(right), &folder.join(left), Some(&parked))?;
        let (from, to) = (wide(&parked)?, wide(&folder.join(left))?);
        // SAFETY: both names are NUL-terminated and outlive the call.
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn rename_no_replace(
        from_dir: &impl Handle,
        from: &OsStr,
        to_dir: &impl Handle,
        to: &OsStr,
    ) -> io::Result<()> {
        rename_no_replace_path(&path_of(from_dir)?.join(from), &path_of(to_dir)?.join(to))
    }

    pub(super) fn rename_no_replace_path(from: &Path, to: &Path) -> io::Result<()> {
        let (from, to) = (wide(from)?, wide(to)?);
        // SAFETY: both names are NUL-terminated and outlive the call; with no
        // flags the call refuses an existing destination.
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
