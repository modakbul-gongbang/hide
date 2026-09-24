//! Saving an editor document without ever discarding a change someone else
//! made, and without ever leaving the original truncated.
//!
//! This is not a compare-and-swap: no filesystem call compares content and
//! replaces a file in one step. It is exchange-and-verify:
//!
//! 1. the file is checked (a regular file, not a symlink, one link, owned by
//!    this user, writable) and its bytes are hashed against the revision the
//!    caller read; a different hash is a conflict and nothing is written;
//! 2. the new bytes go to a temporary file in the same opened folder, with the
//!    original's permissions, and are synced;
//! 3. the temporary file and the original are exchanged in one atomic call
//!    (`RENAME_SWAP` on macOS, `RENAME_EXCHANGE` on Linux), so the path names
//!    either the old file or the new one and never a truncated one;
//! 4. the file that was displaced is hashed again. If it is not the revision
//!    the caller read, someone changed it between step 1 and step 3: the two
//!    are exchanged back, their version stays at the path, and the save is a
//!    conflict. Only when it matches is the displaced file removed.
//!
//! So every change that was complete at the path before the exchange
//! survives, and the operator's draft survives in the caller whatever
//! happens. What this cannot see is a writer that already holds the old file
//! open and writes to it after step 4; a later save of that writer's content
//! will then be judged against the new revision like any other edit.
//! A filesystem that has no atomic exchange is refused before anything is
//! written (PRD S5.5 B13-B15).
//! A process that dies between steps 3 and 4 leaves the displaced original
//! beside the file under its `.hide-save-` name, so it is kept, not lost.
//!
//! Saves and revision reads in one folder hold an advisory lock on that
//! folder, exclusive for a save and shared for a read. Only Hide takes it, so
//! it orders Hide's own work: a revision read that settles a save whose
//! answer was lost (B14) waits for that save to finish, even when the save
//! runs in a helper process whose connection already ended.
//!
//! A file whose bytes already equal the draft is saved whatever revision the
//! caller expected: nothing would change, and a save whose answer was lost
//! and then repeated as a new intent finds its own content there.
//!
//! A symbolic link inside the checkout is saved through: its target, resolved
//! inside the opened root, is the file that is checked and exchanged, and the
//! link stays a link. A link that leaves the root is refused by the handle.

use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::Path;

use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};

use crate::document::{MAX_EDITABLE_BYTES, read_bounded, revision_of};
use crate::error::{ErrorCode, HostError, HostResult};
use crate::root::open_parent;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub revision: String,
    pub modified_at_unix_ms: u64,
}

/// The points a test can stop a save at to change the world underneath it.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveStage {
    /// The original was checked and the temporary file is written; the
    /// exchange has not happened.
    BeforeExchange,
}

/// Saves `contents` to `relative` under `dir` if the file there still holds
/// `expected_revision`.
pub fn save(
    dir: &Dir,
    relative: &Path,
    contents: &[u8],
    expected_revision: &str,
) -> HostResult<Saved> {
    save_observed(
        dir,
        relative,
        contents,
        expected_revision,
        &mut |_, _, _| {},
    )
}

#[doc(hidden)]
pub fn save_observed(
    dir: &Dir,
    relative: &Path,
    contents: &[u8],
    expected_revision: &str,
    observe: &mut dyn FnMut(SaveStage, &Dir, &OsStr),
) -> HostResult<Saved> {
    if contents.len() as u64 > MAX_EDITABLE_BYTES {
        return Err(HostError::new(
            ErrorCode::TooLarge,
            "The document is larger than the editable size; the draft was preserved",
        ));
    }
    let resolved = resolve_link(dir, relative)?;
    let (parent, name) = open_parent(dir, &resolved)?;
    let _lock = FolderLock::acquire(&parent, true)?;
    let original = inspect_target(&parent, &name)?;
    let current = read_revision(&parent, &name)?;
    if current == revision_of(contents) {
        return Ok(Saved {
            revision: current,
            modified_at_unix_ms: original.modified_at_unix_ms,
        });
    }
    if current != expected_revision {
        return Err(HostError::conflict(
            Some(current),
            "The file changed on disk; choose Reload or Keep Editing",
        ));
    }

    let temporary = write_temporary(&parent, &name, contents, original.mode)?;
    observe(SaveStage::BeforeExchange, &parent, &name);
    if let Err(error) = exchange(&parent, &temporary, &name) {
        let _ = parent.remove_file(Path::new(&temporary));
        return Err(
            if matches!(error.raw_os_error(), Some(code) if exchange_unsupported(code)) {
                HostError::new(
                    ErrorCode::Unsupported,
                    "This filesystem cannot replace a file atomically, so the file was not saved; the draft was preserved",
                )
            } else {
                HostError::io(
                    &error,
                    "The file could not be saved; the draft was preserved",
                )
            },
        );
    }

    // The displaced file now sits under the temporary name.
    let displaced = read_revision(&parent, &temporary);
    if displaced.as_deref().ok() != Some(expected_revision) {
        let actual = displaced.ok();
        return match exchange(&parent, &temporary, &name) {
            Ok(()) => {
                let _ = parent.remove_file(Path::new(&temporary));
                Err(HostError::conflict(
                    actual,
                    "The file changed on disk while it was being saved; the other version was kept and the draft was preserved",
                ))
            }
            Err(error) => Err(HostError::new(
                ErrorCode::Conflict,
                format!(
                    "The file changed on disk while it was being saved and could not be put back ({error}); the other version is kept beside it as {}, and the draft was preserved",
                    temporary.to_string_lossy()
                ),
            )),
        };
    }
    parent.remove_file(Path::new(&temporary)).map_err(|error| {
        HostError::io(
            &error,
            format!(
                "The file was saved but its previous version could not be removed from {}",
                temporary.to_string_lossy()
            ),
        )
    })?;
    sync_directory(&parent);
    let saved = parent
        .symlink_metadata(Path::new(&name))
        .map_err(|error| HostError::io(&error, "The saved file could not be inspected"))?;
    Ok(Saved {
        revision: revision_of(contents),
        modified_at_unix_ms: modified_of(&saved),
    })
}

struct Target {
    mode: u32,
    modified_at_unix_ms: u64,
}

fn modified_of(metadata: &cap_std::fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.into_std().duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// Refuses every target a replace-by-exchange could not save faithfully.
fn inspect_target(parent: &Dir, name: &OsStr) -> HostResult<Target> {
    let metadata = parent.symlink_metadata(Path::new(name)).map_err(|error| {
        HostError::io(
            &error,
            "The existing file could not be inspected; the draft was preserved",
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(HostError::new(
            ErrorCode::Unsupported,
            "The file is a symbolic link, so saving would replace the link; open its target instead. The draft was preserved",
        ));
    }
    if !metadata.is_file() {
        return Err(HostError::new(
            ErrorCode::NotAFile,
            "The save target is no longer a regular file; the draft was preserved",
        ));
    }
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        if metadata.nlink() > 1 {
            return Err(HostError::new(
                ErrorCode::Unsupported,
                "The file has other hard links, which a save would separate from it; the draft was preserved",
            ));
        }
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(HostError::new(
                ErrorCode::PermissionDenied,
                "The file belongs to another user, and saving would change its owner; the draft was preserved",
            ));
        }
        if metadata.mode() & 0o200 == 0 {
            return Err(HostError::new(
                ErrorCode::PermissionDenied,
                "The file is read-only on disk; the draft was preserved",
            ));
        }
        Ok(Target {
            mode: metadata.mode() & 0o7777,
            modified_at_unix_ms: modified_of(&metadata),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        Err(HostError::new(
            ErrorCode::Unsupported,
            "Saving is not supported on this platform; the draft was preserved",
        ))
    }
}

fn read_revision(parent: &Dir, name: &OsStr) -> HostResult<String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_NOFOLLOW);
    }
    let mut file = parent
        .open_with(Path::new(name), &options)
        .map_err(|error| {
            HostError::io(
                &error,
                "The existing file could not be read; the draft was preserved",
            )
        })?
        .into_std();
    let bytes = read_bounded(&mut file)?.ok_or_else(|| {
        HostError::new(
            ErrorCode::TooLarge,
            "The file grew beyond the editable size; the draft was preserved",
        )
    })?;
    Ok(revision_of(&bytes))
}

fn write_temporary(parent: &Dir, name: &OsStr, contents: &[u8], mode: u32) -> HostResult<OsString> {
    let mut last_error = None;
    for attempt in 0..8u32 {
        let mut temporary = OsString::from(".");
        temporary.push(name);
        temporary.push(format!(".hide-save-{}-{attempt}", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(mode);
        }
        match parent.open_with(Path::new(&temporary), &options) {
            Ok(file) => {
                let mut file = file.into_std();
                let written = (|| {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        // create applies the umask; the saved file keeps the
                        // original's exact permissions.
                        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
                    }
                    file.write_all(contents)?;
                    file.sync_all()
                })();
                if let Err(error) = written {
                    let _ = parent.remove_file(Path::new(&temporary));
                    return Err(HostError::io(
                        &error,
                        "The file could not be saved; the draft was preserved",
                    ));
                }
                return Ok(temporary);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => last_error = Some(error),
            Err(error) => {
                return Err(HostError::io(
                    &error,
                    "The folder is not writable, so the file could not be saved safely; the draft was preserved",
                ));
            }
        }
    }
    Err(HostError::io(
        &last_error.unwrap_or_else(|| io::Error::from(io::ErrorKind::AlreadyExists)),
        "A temporary save file could not be created; the draft was preserved",
    ))
}

#[cfg(target_os = "macos")]
fn exchange(parent: &Dir, left: &OsStr, right: &OsStr) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let left = CString::new(left.as_bytes())?;
    let right = CString::new(right.as_bytes())?;
    let fd = parent.as_raw_fd();
    let result =
        unsafe { libc::renameatx_np(fd, left.as_ptr(), fd, right.as_ptr(), libc::RENAME_SWAP) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn exchange(parent: &Dir, left: &OsStr, right: &OsStr) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let left = CString::new(left.as_bytes())?;
    let right = CString::new(right.as_bytes())?;
    let fd = parent.as_raw_fd();
    let result =
        unsafe { libc::renameat2(fd, left.as_ptr(), fd, right.as_ptr(), libc::RENAME_EXCHANGE) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn exchange(_parent: &Dir, _left: &OsStr, _right: &OsStr) -> io::Result<()> {
    Err(io::Error::from_raw_os_error(libc::ENOTSUP))
}

fn exchange_unsupported(code: i32) -> bool {
    code == libc::ENOTSUP
        || code == libc::EINVAL
        || code == libc::ENOSYS
        || code == libc::EOPNOTSUPP
}

fn sync_directory(parent: &Dir) {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        unsafe {
            libc::fsync(parent.as_raw_fd());
        }
    }
}

/// The content revision of `relative` now, read the same way a save reads it
/// and after any save in progress in its folder has finished.
pub fn current_revision(dir: &Dir, relative: &Path) -> HostResult<String> {
    let resolved = resolve_link(dir, relative)?;
    let (parent, name) = open_parent(dir, &resolved)?;
    let _lock = FolderLock::acquire(&parent, false)?;
    read_revision(&parent, &name)
}

/// `relative` itself, or when it names a symbolic link, the file the link
/// resolves to inside `dir`. `Dir::canonicalize` answers a path relative to
/// the handle and refuses one that leaves it.
fn resolve_link(dir: &Dir, relative: &Path) -> HostResult<std::path::PathBuf> {
    let is_link = dir
        .symlink_metadata(relative)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);
    if !is_link {
        return Ok(relative.to_path_buf());
    }
    dir.canonicalize(relative).map_err(|error| {
        HostError::io(
            &error,
            "The link's target could not be resolved inside the checkout; the draft was preserved",
        )
    })
}

/// How long a save waits for another of Hide's saves in the same folder
/// before it answers busy instead of queueing forever.
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// The folder's advisory lock, released when dropped (or when the process
/// holding it ends).
struct FolderLock<'a> {
    #[cfg_attr(not(unix), allow(dead_code))]
    folder: &'a Dir,
}

impl<'a> FolderLock<'a> {
    fn acquire(folder: &'a Dir, exclusive: bool) -> HostResult<Self> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let operation = if exclusive {
                libc::LOCK_EX
            } else {
                libc::LOCK_SH
            } | libc::LOCK_NB;
            let deadline = std::time::Instant::now() + LOCK_WAIT;
            loop {
                if unsafe { libc::flock(folder.as_raw_fd(), operation) } == 0 {
                    return Ok(Self { folder });
                }
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::EWOULDBLOCK) {
                    return Err(HostError::io(
                        &error,
                        "The folder could not be locked for the save; the draft was preserved",
                    ));
                }
                if std::time::Instant::now() >= deadline {
                    return Err(HostError::new(
                        ErrorCode::Busy,
                        "Another save in this folder has not finished; the draft was preserved",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        #[cfg(not(unix))]
        {
            let _ = exclusive;
            Ok(Self { folder })
        }
    }
}

impl Drop for FolderLock<'_> {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            unsafe {
                libc::flock(self.folder.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}
