//! Changes under a folder, reported the same way on every system.
//!
//! A [`Watcher`] watches folders, each with everything under it, and puts
//! what changed on its [`Changes`] queue: the path that was created, written,
//! renamed or removed, once however many times the system reported it while
//! it waited. Reading a file is not a change. The queue is bounded: when more
//! paths wait than it holds, or the system's own queue overflowed, or the
//! system stopped watching, the waiting paths give way to one
//! [`Change::Overflow`], which says anything under a watched folder may have
//! changed. A burst therefore costs a bounded number of changes and is never
//! silence.
//!
//! macOS (FSEvents) and Linux (inotify) are watched through `notify`, with
//! its event kinds folded into the one [`Change`]: inotify reports a read as
//! an access event, which is dropped, and FSEvents reports the folder's real
//! path, which is put back under the spelling the caller watched. Windows is
//! watched here with `ReadDirectoryChangesW`, because `notify` drops the
//! answer Windows gives when its buffer overflows (no entries, or
//! `ERROR_NOTIFY_ENUM_DIR`, on which it also stops watching), which would be
//! silence.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How many changed paths wait before they give way to an overflow.
pub const CAPACITY: usize = 4096;

/// One report on a [`Changes`] queue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Change {
    /// Something at `path` or under it was created, written, renamed or
    /// removed; `at` is the last time the system said so.
    Path { path: PathBuf, at: Instant },
    /// Changes were lost: anything under any watched folder may have
    /// changed, and the paths that were waiting are folded into this one.
    /// `reason` says why, for the caller's log.
    Overflow { reason: String, at: Instant },
}

/// Watches folders and reports what changes under them on one queue.
/// Dropping it stops every watch.
pub struct Watcher {
    inner: sys::Inner,
}

impl Watcher {
    /// A watcher with no folder yet, and the queue it reports on.
    pub fn new() -> io::Result<(Self, Changes)> {
        let queue = Arc::new(Queue::default());
        let inner = sys::Inner::new(Arc::clone(&queue))?;
        Ok((Self { inner }, Changes { queue }))
    }

    /// Watches the folder `dir` and everything under it. A path it reports
    /// starts with `dir` as spelled here, whatever the system calls it.
    pub fn watch(&mut self, dir: &Path) -> io::Result<()> {
        if !std::fs::metadata(dir)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "only a folder can be watched",
            ));
        }
        self.inner.watch(dir)
    }

    /// Stops watching `dir`, spelled as it was watched.
    pub fn unwatch(&mut self, dir: &Path) -> io::Result<()> {
        self.inner.unwatch(dir)
    }
}

/// The queue a [`Watcher`] reports on.
pub struct Changes {
    queue: Arc<Queue>,
}

impl Changes {
    /// The oldest waiting change, if any.
    pub fn try_recv(&self) -> Option<Change> {
        self.queue.lock().pop()
    }

    /// The oldest waiting change, waiting at most `timeout` for one.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Change> {
        let deadline = Instant::now() + timeout;
        let mut state = self.queue.lock();
        loop {
            if let Some(change) = state.pop() {
                return Some(change);
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            state = self
                .queue
                .ready
                .wait_timeout(state, left)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
    }
}

#[derive(Default)]
struct Queue {
    state: Mutex<State>,
    ready: Condvar,
}

#[derive(Default)]
struct State {
    order: VecDeque<PathBuf>,
    waiting: HashMap<PathBuf, Instant>,
    overflow: Option<(String, Instant)>,
}

impl Queue {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn changed(&self, path: PathBuf) {
        let now = Instant::now();
        let mut state = self.lock();
        if let Some((_, at)) = state.overflow.as_mut() {
            *at = now;
        } else if let Some(at) = state.waiting.get_mut(&path) {
            *at = now;
        } else if state.order.len() < CAPACITY {
            state.order.push_back(path.clone());
            state.waiting.insert(path, now);
        } else {
            drop(state);
            self.lost(format!("more than {CAPACITY} changed paths waited"));
            return;
        }
        drop(state);
        self.ready.notify_all();
    }

    fn lost(&self, reason: String) {
        let mut state = self.lock();
        state.order.clear();
        state.waiting.clear();
        state.overflow = Some((reason, Instant::now()));
        drop(state);
        self.ready.notify_all();
    }
}

impl State {
    fn pop(&mut self) -> Option<Change> {
        if let Some((reason, at)) = self.overflow.take() {
            return Some(Change::Overflow { reason, at });
        }
        let path = self.order.pop_front()?;
        let at = self.waiting.remove(&path)?;
        Some(Change::Path { path, at })
    }
}

#[cfg(unix)]
mod sys {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use notify::event::{AccessKind, AccessMode, MetadataKind, ModifyKind};
    use notify::{EventKind, RecursiveMode, Watcher as _};

    use super::Queue;

    /// A watched folder as the caller spelled it, and as the system reports
    /// it (FSEvents answers with the real path, `/private/var` for `/var`).
    struct Root {
        watched: PathBuf,
        real: PathBuf,
    }

    pub(super) struct Inner {
        watcher: notify::RecommendedWatcher,
        roots: Arc<Mutex<Vec<Root>>>,
    }

    impl Inner {
        pub(super) fn new(queue: Arc<Queue>) -> io::Result<Self> {
            let roots = Arc::new(Mutex::new(Vec::<Root>::new()));
            let seen = Arc::clone(&roots);
            let watcher =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    let event = match event {
                        Ok(event) => event,
                        Err(error) => return queue.lost(format!("the watch failed: {error}")),
                    };
                    if event.need_rescan() {
                        return queue.lost("the system's queue of changes overflowed".to_owned());
                    }
                    if is_read(&event.kind) {
                        return;
                    }
                    let roots = seen.lock().unwrap_or_else(|error| error.into_inner());
                    for path in event.paths {
                        queue.changed(as_watched(&roots, path));
                    }
                })
                .map_err(into_io)?;
            Ok(Self { watcher, roots })
        }

        pub(super) fn watch(&mut self, dir: &Path) -> io::Result<()> {
            let real = std::fs::canonicalize(dir)?;
            self.watcher
                .watch(dir, RecursiveMode::Recursive)
                .map_err(into_io)?;
            let mut roots = self.roots.lock().unwrap_or_else(|error| error.into_inner());
            roots.retain(|root| root.watched != dir);
            roots.push(Root {
                watched: dir.to_path_buf(),
                real,
            });
            Ok(())
        }

        pub(super) fn unwatch(&mut self, dir: &Path) -> io::Result<()> {
            self.roots
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .retain(|root| root.watched != dir);
            self.watcher.unwatch(dir).map_err(into_io)
        }
    }

    /// A read: inotify reports opening, reading and closing a file as
    /// access (closing after a write is the one access that follows a
    /// change), and a system that keeps access times may report that a read
    /// moved one.
    fn is_read(kind: &EventKind) -> bool {
        match kind {
            EventKind::Access(kind) => !matches!(kind, AccessKind::Close(AccessMode::Write)),
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)) => true,
            _ => false,
        }
    }

    /// `path` under the spelling of the watched folder it lies in.
    fn as_watched(roots: &[Root], path: PathBuf) -> PathBuf {
        for root in roots {
            if path.starts_with(&root.watched) {
                return path;
            }
            if let Ok(rest) = path.strip_prefix(&root.real) {
                return root.watched.join(rest);
            }
        }
        path
    }

    fn into_io(error: notify::Error) -> io::Error {
        match error.kind {
            notify::ErrorKind::Io(error) => error,
            notify::ErrorKind::PathNotFound => {
                io::Error::new(io::ErrorKind::NotFound, "the folder does not exist")
            }
            notify::ErrorKind::MaxFilesWatch => io::Error::new(
                io::ErrorKind::QuotaExceeded,
                "the system's limit on watched folders was reached",
            ),
            other => io::Error::other(format!("{other:?}")),
        }
    }
}

#[cfg(windows)]
mod sys {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::io;
    use std::os::windows::ffi::OsStringExt;
    use std::path::{Path, PathBuf};
    use std::ptr::{null, null_mut};
    use std::sync::Arc;
    use std::thread::JoinHandle;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_NOTIFY_ENUM_DIR, ERROR_OPERATION_ABORTED, HANDLE, INVALID_HANDLE_VALUE,
        WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
        FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
        FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_NOTIFY_INFORMATION,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadDirectoryChangesW,
    };
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Threading::{
        CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects,
    };

    use super::Queue;
    use crate::fs::wide;

    /// What a change is: a name, a size or a write. Not access, so a read is
    /// never one; not attributes or security, which no caller asks about.
    const FILTER: u32 = FILE_NOTIFY_CHANGE_FILE_NAME
        | FILE_NOTIFY_CHANGE_DIR_NAME
        | FILE_NOTIFY_CHANGE_SIZE
        | FILE_NOTIFY_CHANGE_LAST_WRITE
        | FILE_NOTIFY_CHANGE_CREATION;
    /// The buffer one read fills, in `u32`s so the entries are aligned.
    /// Windows keeps changes in a buffer of the same size between reads.
    const BUFFER_WORDS: usize = 16 * 1024;

    /// A kernel handle closed on drop, which a watch thread may own.
    struct Owned(HANDLE);

    // SAFETY: a kernel handle may be used and closed from any thread.
    unsafe impl Send for Owned {}

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: the handle is open and owned by this value.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// One watched folder: the event that stops its thread, and the thread.
    struct Watch {
        stop: Owned,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for Watch {
        fn drop(&mut self) {
            // SAFETY: the event is open; the thread waits on it and cancels
            // its read before it returns.
            unsafe { SetEvent(self.stop.0) };
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    pub(super) struct Inner {
        queue: Arc<Queue>,
        watches: HashMap<PathBuf, Watch>,
    }

    fn event() -> io::Result<Owned> {
        // SAFETY: a manual-reset, unnamed, unsignalled event.
        let handle = unsafe { CreateEventW(null(), 1, 0, null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Owned(handle))
    }

    impl Inner {
        pub(super) fn new(queue: Arc<Queue>) -> io::Result<Self> {
            Ok(Self {
                queue,
                watches: HashMap::new(),
            })
        }

        pub(super) fn watch(&mut self, dir: &Path) -> io::Result<()> {
            let name = wide(dir)?;
            // SAFETY: `name` is NUL-terminated. Every sharing mode is allowed,
            // so the watch never stops anyone renaming or deleting the folder.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    FILE_LIST_DIRECTORY,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                    null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let folder = Owned(handle);
            let done = event()?;
            let stop = event()?;
            let stop_for_thread = stop.0 as usize;
            let queue = Arc::clone(&self.queue);
            let root = dir.to_path_buf();
            let thread = std::thread::Builder::new()
                .name("hide-watch".to_owned())
                .spawn(move || {
                    read_changes(&folder, &done, stop_for_thread as HANDLE, &root, &queue)
                })?;
            self.watches.insert(
                dir.to_path_buf(),
                Watch {
                    stop,
                    thread: Some(thread),
                },
            );
            Ok(())
        }

        pub(super) fn unwatch(&mut self, dir: &Path) -> io::Result<()> {
            match self.watches.remove(dir) {
                Some(_) => Ok(()),
                None => Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "the folder is not watched",
                )),
            }
        }
    }

    /// Reads changes until `stop` is set, putting each on `queue`. A read
    /// that comes back empty, or with `ERROR_NOTIFY_ENUM_DIR`, means the
    /// system's buffer overflowed and its changes are lost.
    fn read_changes(folder: &Owned, done: &Owned, stop: HANDLE, root: &Path, queue: &Queue) {
        let mut buffer = vec![0u32; BUFFER_WORDS];
        loop {
            // SAFETY: an all-zero OVERLAPPED is its documented initial state.
            let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
            overlapped.hEvent = done.0;
            // SAFETY: the buffer and the OVERLAPPED outlive the read, which
            // completes (or is cancelled and waited for) before either is
            // touched again.
            let started = unsafe {
                ReadDirectoryChangesW(
                    folder.0,
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * 4) as u32,
                    1,
                    FILTER,
                    null_mut(),
                    &mut overlapped,
                    None,
                )
            };
            if started == 0 {
                let error = io::Error::last_os_error();
                return queue.lost(format!("the system stopped watching: {error}"));
            }
            let handles = [done.0, stop];
            // SAFETY: both handles are open events.
            let woke = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
            let mut length = 0u32;
            if woke != WAIT_OBJECT_0 {
                // SAFETY: the read is pending on this handle; it is cancelled
                // and waited for, so the buffer is free when this returns.
                unsafe {
                    CancelIoEx(folder.0, &overlapped);
                    GetOverlappedResult(folder.0, &overlapped, &mut length, 1);
                }
                return;
            }
            // SAFETY: the read completed, so its result is ready.
            if unsafe { GetOverlappedResult(folder.0, &overlapped, &mut length, 0) } == 0 {
                let error = io::Error::last_os_error();
                match error.raw_os_error().map(|code| code as u32) {
                    Some(ERROR_NOTIFY_ENUM_DIR) => {
                        queue.lost("the system's buffer of changes overflowed".to_owned());
                        continue;
                    }
                    Some(ERROR_OPERATION_ABORTED) => return,
                    _ => return queue.lost(format!("the system stopped watching: {error}")),
                }
            }
            if length == 0 {
                queue.lost("the system's buffer of changes overflowed".to_owned());
                continue;
            }
            for name in entries(&buffer, length as usize) {
                queue.changed(root.join(name));
            }
        }
    }

    /// The names in the first `length` bytes of a filled buffer.
    fn entries(buffer: &[u32], length: usize) -> Vec<PathBuf> {
        let bytes = buffer.as_ptr().cast::<u8>();
        let header = std::mem::offset_of!(FILE_NOTIFY_INFORMATION, FileName);
        let mut names = Vec::new();
        let mut offset = 0usize;
        while offset + header <= length {
            // SAFETY: the entry starts inside the filled part of the buffer,
            // at an offset the system aligned to four bytes.
            let entry = unsafe { &*bytes.add(offset).cast::<FILE_NOTIFY_INFORMATION>() };
            let units = entry.FileNameLength as usize / 2;
            if offset + header + units * 2 > length {
                break;
            }
            // SAFETY: the name's UTF-16 units follow the header inside the
            // filled part of the buffer, as checked above.
            let name = unsafe {
                std::slice::from_raw_parts(bytes.add(offset + header).cast::<u16>(), units)
            };
            names.push(PathBuf::from(OsString::from_wide(name)));
            if entry.NextEntryOffset == 0 {
                break;
            }
            offset += entry.NextEntryOffset as usize;
        }
        names
    }
}
