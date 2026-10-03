//! How much of its volume a file takes, and how much the volume has left.
//!
//! What a file takes is what the filesystem allocated for it, not its length:
//! a sparse or compressed file takes less, a small file a whole block. A
//! name with several links takes its space once, so a walk that adds files
//! up counts each [`Usage::id`] once.

use std::io;
use std::path::Path;

use super::identity::FileId;

/// What one entry takes, read without following a link at its name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Usage {
    /// Bytes the filesystem allocated for the entry.
    pub allocated: u64,
    /// How many names the entry has.
    pub links: u64,
    /// The entry's identity, which a second name of it shares.
    pub id: FileId,
    /// Whether the entry is a folder. A link to a folder is not one.
    pub is_dir: bool,
}

/// What the entry `path` names takes, a link's own and not its target's. On
/// Unix it is one `lstat`; Windows has no call that answers it from a name,
/// so the entry is opened (asking no access) and asked.
pub fn usage_nofollow(path: &Path) -> io::Result<Usage> {
    sys::usage_nofollow(path)
}

/// Bytes an unprivileged writer can still put on the volume holding `path`,
/// after any quota the account has.
pub fn free_bytes(path: &Path) -> io::Result<u64> {
    sys::free_bytes(path)
}

#[cfg(unix)]
mod sys {
    use std::io;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    use super::{FileId, Usage};

    pub(super) fn usage_nofollow(path: &Path) -> io::Result<Usage> {
        let metadata = std::fs::symlink_metadata(path)?;
        Ok(Usage {
            // `st_blocks` counts 512-byte units on every Unix this runs on.
            allocated: metadata.blocks().saturating_mul(512),
            links: metadata.nlink(),
            id: FileId::new(metadata.dev(), u128::from(metadata.ino())),
            is_dir: metadata.is_dir(),
        })
    }

    // The block count is 32-bit on macOS and 64-bit elsewhere.
    #[allow(clippy::useless_conversion)]
    pub(super) fn free_bytes(path: &Path) -> io::Result<u64> {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a path holds a NUL"))?;
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: `name` is NUL-terminated and `stat` is a writable statvfs.
        if unsafe { libc::statvfs(name.as_ptr(), stat.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call succeeded and filled the structure.
        let stat = unsafe { stat.assume_init() };
        Ok(u64::from(stat.f_bavail).saturating_mul(u64::from(stat.f_frsize)))
    }
}

#[cfg(windows)]
mod sys {
    use std::io;
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use std::ptr::null_mut;

    use windows_sys::Win32::Storage::FileSystem::{
        FILE_STANDARD_INFO, FileStandardInfo, GetDiskFreeSpaceExW, GetFileInformationByHandleEx,
    };

    use super::Usage;
    use crate::fs::{identity, open_for_query, wide};

    pub(super) fn usage_nofollow(path: &Path) -> io::Result<Usage> {
        let entry = open_for_query(path, false)?;
        let mut info = MaybeUninit::<FILE_STANDARD_INFO>::zeroed();
        // SAFETY: the handle is open for the call, and `info` is a writable
        // FILE_STANDARD_INFO of the size passed.
        if unsafe {
            GetFileInformationByHandleEx(
                entry.as_raw_handle(),
                FileStandardInfo,
                info.as_mut_ptr().cast(),
                size_of::<FILE_STANDARD_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call succeeded and filled the structure.
        let info = unsafe { info.assume_init() };
        // A link opened as itself is a reparse point, never a folder.
        let is_link = entry.metadata()?.file_type().is_symlink();
        Ok(Usage {
            allocated: u64::try_from(info.AllocationSize).unwrap_or(0),
            links: u64::from(info.NumberOfLinks),
            id: identity::file_id_of(&entry)?,
            is_dir: info.Directory && !is_link,
        })
    }

    pub(super) fn free_bytes(path: &Path) -> io::Result<u64> {
        let name = wide(path)?;
        let mut available = 0u64;
        // SAFETY: `name` is NUL-terminated, `available` is writable, and the
        // totals the call can also fill are not asked for.
        if unsafe { GetDiskFreeSpaceExW(name.as_ptr(), &mut available, null_mut(), null_mut()) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(available)
    }
}
