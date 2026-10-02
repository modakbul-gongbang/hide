//! What an open file allows its owner, kept across a replacement.

use std::fs;
use std::io;
use std::path::Path;

use super::Handle;

/// The permissions of one file as a replacement has to keep them: the mode
/// bits on Unix, the read-only flag on Windows (whose access list a
/// replacement keeps by replacing through the system's own call, see
/// [`super::atomic`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Permissions {
    inner: sys::Inner,
}

impl Permissions {
    /// The permissions of the open file.
    pub fn of(handle: &impl Handle) -> io::Result<Self> {
        Ok(Self {
            inner: sys::of(handle)?,
        })
    }

    /// Whether the owner may change the file's contents.
    pub fn owner_can_write(&self) -> bool {
        sys::owner_can_write(&self.inner)
    }

    /// Gives `file` exactly these permissions, whatever the creating process's
    /// umask took off.
    pub fn apply(&self, file: &fs::File) -> io::Result<()> {
        sys::apply(&self.inner, file)
    }

    /// The mode bits, `None` where the system has none.
    pub fn unix_mode(&self) -> Option<u32> {
        sys::unix_mode(&self.inner)
    }
}

/// Whether the file at `path` is one the system will run. Unix asks for an
/// execute bit; Windows has no such bit and runs a file by its extension, so
/// it asks for one of the extensions the shell launches.
pub fn is_executable(path: &Path) -> io::Result<bool> {
    sys::is_executable(path)
}

#[cfg(unix)]
mod sys {
    use std::fs;
    use std::io;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::Path;

    use super::Handle;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(super) struct Inner(u32);

    pub(super) fn of(handle: &impl Handle) -> io::Result<Inner> {
        let metadata = crate::fs::duplicate(handle)?.metadata()?;
        Ok(Inner(metadata.mode() & 0o7777))
    }

    pub(super) fn owner_can_write(inner: &Inner) -> bool {
        inner.0 & 0o200 != 0
    }

    pub(super) fn apply(inner: &Inner, file: &fs::File) -> io::Result<()> {
        file.set_permissions(fs::Permissions::from_mode(inner.0))
    }

    pub(super) fn unix_mode(inner: &Inner) -> Option<u32> {
        Some(inner.0)
    }

    pub(super) fn is_executable(path: &Path) -> io::Result<bool> {
        Ok(fs::metadata(path)?.permissions().mode() & 0o111 != 0)
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::OsStr;
    use std::fs;
    use std::io;
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;

    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_READONLY, GetFileInformationByHandle,
    };

    use super::Handle;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(super) struct Inner {
        read_only: bool,
    }

    pub(super) fn of(handle: &impl Handle) -> io::Result<Inner> {
        let raw = handle.as_handle().as_raw_handle();
        let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
        // SAFETY: `raw` is an open handle borrowed for the call and `info` is
        // a writable structure of the size the call expects.
        if unsafe { GetFileInformationByHandle(raw, info.as_mut_ptr()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call succeeded and filled the structure.
        let attributes = unsafe { info.assume_init() }.dwFileAttributes;
        Ok(Inner {
            read_only: attributes & FILE_ATTRIBUTE_READONLY != 0,
        })
    }

    pub(super) fn owner_can_write(inner: &Inner) -> bool {
        !inner.read_only
    }

    pub(super) fn apply(inner: &Inner, file: &fs::File) -> io::Result<()> {
        let mut permissions = file.metadata()?.permissions();
        permissions.set_readonly(inner.read_only);
        file.set_permissions(permissions)
    }

    pub(super) fn unix_mode(_: &Inner) -> Option<u32> {
        None
    }

    /// The extensions `CreateProcess` and the shell run directly.
    const RUNNABLE: [&str; 6] = ["exe", "com", "bat", "cmd", "ps1", "msc"];

    pub(super) fn is_executable(path: &Path) -> io::Result<bool> {
        fs::metadata(path)?;
        Ok(path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| {
                RUNNABLE
                    .iter()
                    .any(|runnable| extension.eq_ignore_ascii_case(runnable))
            }))
    }
}
