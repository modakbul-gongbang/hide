//! Advisory locks on files and folders, held by an open descriptor.
//!
//! The lock lives as long as the [`Lock`] and ends with the process that
//! holds it, so a crash never leaves one behind. On Unix it belongs to the
//! open file, not the descriptor: a child started while the lock is held has
//! a copy of the descriptor until it starts its program (descriptors are
//! close-on-exec), so a drop in that moment frees the lock when the child
//! starts its program, not at once. It orders cooperating
//! programs and stops nobody else from opening the file. A shared lock admits
//! other shared locks; an exclusive lock admits none.
//!
//! A folder can be locked as well as a file ([`lock_dir`]), through a
//! descriptor the system can lock, which is not always the one a caller
//! holds. On Unix `flock` takes a folder, but `cap-std` opens its folders with
//! `O_PATH` on Linux and the kernel answers `EBADF` to a lock or a flush on
//! those, so the folder is locked through a fresh descriptor of the same
//! folder. Windows refuses a byte-range lock on a folder altogether, so the
//! lock is on a file of the account's own that stands for the folder.

use std::fs::{self, File};
use std::io;
use std::time::{Duration, Instant};

use super::Handle;

/// How long a wait sleeps between two tries.
const POLL: Duration = Duration::from_millis(20);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Shared,
    Exclusive,
}

/// A lock held on a file or folder; dropping it releases the lock.
#[derive(Debug)]
pub struct Lock {
    _file: File,
}

/// How a wait for a lock ended.
#[derive(Debug)]
#[must_use = "a wait that timed out or was cancelled holds no lock"]
pub enum Waited {
    Locked(Lock),
    /// Another holder still had it when the time ran out.
    TimedOut,
    /// `cancelled` answered yes before the lock came free.
    Cancelled,
}

/// Locks `file`, trying again every few milliseconds until `within` has
/// passed or `cancelled` says to stop. A `within` of zero is one try. The
/// lock is held on the descriptor, so the file is kept open for as long as
/// the lock is.
#[allow(clippy::disallowed_methods)] // a production wait, not test code
pub fn lock_file(
    file: File,
    mode: Mode,
    within: Duration,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<Waited> {
    let deadline = Instant::now() + within;
    loop {
        if attempt(&file, mode)? {
            return Ok(Waited::Locked(Lock { _file: file }));
        }
        if cancelled() {
            return Ok(Waited::Cancelled);
        }
        if Instant::now() >= deadline {
            return Ok(Waited::TimedOut);
        }
        std::thread::sleep(POLL);
    }
}

/// Locks the open folder `dir` the way [`lock_file`] locks a file.
pub fn lock_dir(
    dir: &impl Handle,
    mode: Mode,
    within: Duration,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<Waited> {
    lock_file(sys::lock_target(dir)?, mode, within, cancelled)
}

/// One try. `Ok(false)` is "somebody else holds it".
fn attempt(file: &File, mode: Mode) -> io::Result<bool> {
    let tried = match mode {
        Mode::Shared => file.try_lock_shared(),
        Mode::Exclusive => file.try_lock(),
    };
    match tried {
        Ok(()) => Ok(true),
        Err(fs::TryLockError::WouldBlock) => Ok(false),
        Err(fs::TryLockError::Error(error)) => Err(error),
    }
}

#[cfg(unix)]
mod sys {
    use std::fs::File;
    use std::io;

    use super::Handle;

    pub(super) fn lock_target(dir: &impl Handle) -> io::Result<File> {
        crate::fs::reopen_dir_for_io(dir)
    }
}

#[cfg(windows)]
mod sys {
    use std::fs::File;
    use std::io;
    use std::path::PathBuf;

    use super::Handle;
    use crate::fs::{identity, private};

    /// How many lock files an account ever has: a folder maps to one of them
    /// by its identity, and two folders that map to the same one only wait
    /// for each other (a task holding one and asking for the other waits for
    /// itself until the wait runs out), never lock wrongly. No cleanup is
    /// needed, because the number cannot grow.
    const STRIPES: u64 = 1 << 16;

    /// Windows refuses a byte-range lock on a folder (`ERROR_ACCESS_DENIED`),
    /// and a lock file inside the folder would put a file in the checkout of
    /// whoever is being saved to. So a folder is locked through a file of its
    /// own that lives with the account's local data, named by the folder's
    /// identity. The lock is therefore one account's: two accounts working in
    /// one folder do not exclude each other here as they do under `flock`.
    pub(super) fn lock_target(dir: &impl Handle) -> io::Result<File> {
        let id = identity::file_id_of(dir)?;
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for byte in id
            .volume()
            .to_le_bytes()
            .into_iter()
            .chain(id.index().to_le_bytes())
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
        let folder = lock_folder()?;
        private::open_or_create_file(&folder.join(format!("{:04x}.lock", hash % STRIPES)))
    }

    /// `<state folder>\hide\locks` (under `%LOCALAPPDATA%`), made private on first use and, when it
    /// was there already, trusted only if the account owns it and nobody else
    /// can change it, since another account that made it first could hold
    /// every lock for ever.
    fn lock_folder() -> io::Result<PathBuf> {
        let mut folder = crate::host::state_dir()?;
        for part in ["hide", "locks"] {
            folder.push(part);
            match private::create_dir(&folder) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if !private::owned_by_current_user(&folder)?
                        || private::others_can_modify(&folder)?
                    {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            format!(
                                "{} can be changed by another account, so no lock is kept there",
                                folder.display()
                            ),
                        ));
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Ok(folder)
    }
}
