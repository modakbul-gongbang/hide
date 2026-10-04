//! Local streams between processes of one account: a Unix domain socket on
//! macOS and Linux, a named pipe on Windows, behind one type.
//!
//! The address is a filesystem path on every system, because that is how
//! Herdr names its socket. On Windows the path is also the pipe's name (the
//! pipe is `\\.\pipe\<path>`, which is how Herdr's own client reaches its
//! server) and a small marker file is kept at the path while the listener
//! lives, so "does the path exist" means the same as it does for a socket
//! file.
//!
//! What a caller can rely on, on all three systems (`tests/ipc.rs` checks
//! each line):
//!
//! - Reads and writes are byte streams; a peer that closes ends a read with
//!   `Ok(0)`.
//! - A read timeout ends a read with `ErrorKind::TimedOut` once the time has
//!   passed; it never returns early with no data.
//! - [`ShutdownHandle::shutdown`] called from another thread ends a read
//!   that is blocked, with `Ok(0)`; later reads return `Ok(0)` and later
//!   writes fail with `BrokenPipe`.
//! - [`LocalListener::bind`] fails with `AddrInUse` while another listener
//!   answers at the path, and replaces what a dead one left behind.
//! - Clients that connect and leave never hold up the next connect, whether
//!   or not anyone is in `accept`, up to the listener's backlog (#315): a
//!   socket's queue on Unix, 64 connections plus the listening instances on
//!   Windows.
//! - [`ListenerCloser::close`] called from another thread ends an `accept`
//!   that is waiting, with `ConnectionAborted`, and every later one.
//! - Only the account that bound the listener can connect to it, and a
//!   client reaches only its own account's listener: a Windows pipe name is
//!   global, so a connect to a pipe another account holds at that name is
//!   refused with `PermissionDenied`.
//!
//! Where the system cannot answer, the call says `ErrorKind::Unsupported`:
//! a Windows pipe has no write timeout.

use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[cfg(unix)]
use interprocess::local_socket::Stream as RawStream;
#[cfg(unix)]
use interprocess::local_socket::prelude::*;
#[cfg(unix)]
use interprocess::local_socket::{GenericFilePath, Listener as RawListener, ListenerOptions, Name};
#[cfg(windows)]
use sys::Pipe as RawStream;

/// What a listener's path may hold when nobody listens: connecting to it is
/// refused or finds nothing. A connect that times out is a listener that
/// answers too slowly, not an absent one.
fn leftover_connect_error(kind: io::ErrorKind) -> bool {
    matches!(
        kind,
        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
    )
}

/// How long a connect waits for a listener that has not yet accepted. A
/// Unix socket never waits; a Windows pipe whose instances are all busy
/// would wait for ever without it.
#[cfg(windows)]
const CONNECT_BOUND: Duration = Duration::from_secs(2);

#[cfg(unix)]
fn endpoint_name(path: &Path) -> io::Result<Name<'_>> {
    path.to_fs_name::<GenericFilePath>()
}

/// The pipe a path names: `\\.\pipe\<path>`, as Herdr's client reaches its
/// server.
#[cfg(windows)]
fn pipe_name(path: &Path) -> String {
    format!(r"\\.\pipe\{}", path.to_string_lossy())
}

fn timeout_nanos(timeout: Option<Duration>) -> io::Result<u64> {
    match timeout {
        None => Ok(0),
        Some(duration) if duration.is_zero() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a zero timeout is not a timeout",
        )),
        Some(duration) => Ok(u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)),
    }
}

fn timeout_of(nanos: &AtomicU64) -> Option<Duration> {
    match nanos.load(Ordering::Relaxed) {
        0 => None,
        nanos => Some(Duration::from_nanos(nanos)),
    }
}

struct Shared {
    raw: RawStream,
    #[cfg(windows)]
    stop: sys::Stop,
}

/// One connected end of a local stream.
pub struct LocalStream {
    shared: Arc<Shared>,
    read_timeout: AtomicU64,
    write_timeout: AtomicU64,
}

impl fmt::Debug for LocalStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalStream")
            .finish_non_exhaustive()
    }
}

impl LocalStream {
    fn from_raw(raw: RawStream) -> Self {
        Self {
            shared: Arc::new(Shared {
                raw,
                #[cfg(windows)]
                stop: sys::Stop::default(),
            }),
            read_timeout: AtomicU64::new(0),
            write_timeout: AtomicU64::new(0),
        }
    }

    /// Connects to the listener at `path`. `NotFound` or `ConnectionRefused`
    /// when nobody answers there; `TimedOut` on Windows when a listener
    /// exists but takes no connection within two seconds.
    pub fn connect(path: &Path) -> io::Result<Self> {
        sys::connect(path).map(Self::from_raw)
    }

    /// Bounds every later read. `None` blocks until data, end of stream or a
    /// shutdown. A zero timeout is `InvalidInput`.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        let nanos = timeout_nanos(timeout)?;
        sys::set_read_timeout(&self.shared, timeout)?;
        self.read_timeout.store(nanos, Ordering::Relaxed);
        Ok(())
    }

    /// Bounds every later write. `Unsupported` on Windows, whose pipes have
    /// none: a request that fits the pipe's buffer never blocks, and
    /// [`ShutdownHandle::shutdown`] frees a writer a stalled peer holds.
    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        let nanos = timeout_nanos(timeout)?;
        sys::set_write_timeout(&self.shared, timeout)?;
        self.write_timeout.store(nanos, Ordering::Relaxed);
        Ok(())
    }

    /// A handle another thread uses to end this stream's blocked reads.
    pub fn shutdown_handle(&self) -> ShutdownHandle {
        ShutdownHandle {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Two connected streams inside this process, for a test double that
    /// stands in for a peer. Nothing outside the process can reach them.
    pub fn pair() -> io::Result<(Self, Self)> {
        sys::pair()
    }

    /// The pid of the process at the other end, when the system reports it.
    pub fn peer_pid(&self) -> io::Result<u32> {
        sys::peer_pid(&self.shared)
    }
}

impl Read for LocalStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        sys::read(&self.shared, timeout_of(&self.read_timeout), buffer)
    }
}

impl Write for LocalStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        sys::write(&self.shared, timeout_of(&self.write_timeout), buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
impl std::os::windows::io::AsRawHandle for LocalStream {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        sys::raw_handle(&self.shared.raw)
    }
}

/// Ends a [`LocalStream`]'s reads and writes from any thread.
pub struct ShutdownHandle {
    shared: Arc<Shared>,
}

impl fmt::Debug for ShutdownHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ShutdownHandle")
            .finish_non_exhaustive()
    }
}

impl ShutdownHandle {
    /// A read blocked on the stream returns `Ok(0)`; later reads return
    /// `Ok(0)` and later writes fail with `BrokenPipe`. Idempotent.
    pub fn shutdown(&self) {
        sys::shutdown(&self.shared);
    }
}

/// A bound endpoint that accepts [`LocalStream`]s.
pub struct LocalListener {
    inner: sys::Listener,
    path: PathBuf,
    #[cfg(windows)]
    marker: String,
}

impl fmt::Debug for LocalListener {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalListener")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl LocalListener {
    /// Binds `path`, private to the current account from the moment it
    /// exists. `AddrInUse` when a listener there still answers; what a dead
    /// listener left at the path is replaced.
    pub fn bind(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        clear_leftover(path)?;
        let inner = sys::Listener::bind(path)?;
        #[cfg(windows)]
        let marker = sys::write_marker(path)?;
        Ok(Self {
            inner,
            path: path.to_path_buf(),
            #[cfg(windows)]
            marker,
        })
    }

    /// Waits for the next connection. A client that connected and left
    /// before it was accepted may still be handed out; its stream reads end
    /// of stream at once. `ConnectionAborted` once the listener is closed.
    pub fn accept(&self) -> io::Result<LocalStream> {
        self.inner.accept().map(LocalStream::from_raw)
    }

    /// A handle another thread uses to end this listener's `accept`s.
    pub fn closer(&self) -> ListenerCloser {
        ListenerCloser {
            closing: self.inner.closing(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Ends a [`LocalListener`]'s `accept`s from any thread.
#[derive(Clone)]
pub struct ListenerCloser {
    closing: Arc<sys::Closing>,
}

impl fmt::Debug for ListenerCloser {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ListenerCloser")
            .finish_non_exhaustive()
    }
}

impl ListenerCloser {
    /// An `accept` waiting on the listener returns `ConnectionAborted`, and
    /// so does every later one. Idempotent. The path stays the listener's
    /// until it is dropped; on Windows the pipe stops taking connects at the
    /// close.
    pub fn close(&self) {
        self.closing.close();
    }
}

fn listener_closed() -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, "the listener was closed")
}

#[cfg(windows)]
impl Drop for LocalListener {
    fn drop(&mut self) {
        // The pipe goes with the handle; the marker is this listener's only
        // trace, removed only while it is still this listener's.
        if fs::read_to_string(&self.path).is_ok_and(|text| text == self.marker) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Whether the entry at `path` has the kind a local listener leaves: a socket
/// on Unix, a marker file on Windows. A link is judged as itself, not followed.
/// This only reads metadata; it neither connects nor removes the entry.
pub fn is_endpoint(path: &Path) -> io::Result<bool> {
    fs::symlink_metadata(path).map(|metadata| sys::is_leftover_kind(&metadata))
}

/// Removes what a dead listener left at `path`, and refuses a live one.
///
/// Only what a listener leaves is removed: a socket on Unix, the marker file
/// on Windows. Anything else at the path is the caller's, and `bind` fails
/// rather than delete it.
fn clear_leftover(path: &Path) -> io::Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if !sys::is_leftover_kind(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists and is not a local endpoint", path.display()),
        ));
    }
    match LocalStream::connect(path) {
        Ok(_) => Err(already_answers(path)),
        Err(error) if error.kind() == io::ErrorKind::TimedOut => Err(already_answers(path)),
        Err(error) if leftover_connect_error(error.kind()) => match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
        Err(error) => Err(error),
    }
}

/// The pid of the process at the other end of a connected Unix socket.
/// `Unsupported` where the system does not report it.
#[cfg(target_os = "macos")]
fn peer_pid_of_fd(socket: &impl std::os::fd::AsFd) -> io::Result<u32> {
    use std::os::fd::AsRawFd;
    let mut pid: libc::pid_t = 0;
    let mut size = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: the output pointers refer to initialized stack values and the
    // descriptor stays owned by `socket` for the whole call.
    let result = unsafe {
        libc::getsockopt(
            socket.as_fd().as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut size,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    u32::try_from(pid)
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(target_os = "linux")]
fn peer_pid_of_fd(socket: &impl std::os::fd::AsFd) -> io::Result<u32> {
    use std::os::fd::AsRawFd;
    // SAFETY: an all-zero `ucred` is a valid value.
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the output pointers are writable for their declared sizes and
    // the descriptor stays owned by `socket` for the whole call.
    let result = unsafe {
        libc::getsockopt(
            socket.as_fd().as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut size,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    u32::try_from(credentials.pid)
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| io::Error::from(io::ErrorKind::Unsupported))
}

fn already_answers(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AddrInUse,
        format!("a listener already answers at {}", path.display()),
    )
}

#[cfg(unix)]
mod sys {
    use std::net::Shutdown;
    use std::os::fd::{AsFd, AsRawFd};
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;

    use interprocess::local_socket::ListenerNonblockingMode;
    use interprocess::os::unix::local_socket::ListenerOptionsExt as _;

    use super::*;

    pub(super) fn connect(path: &Path) -> io::Result<RawStream> {
        RawStream::connect(endpoint_name(path)?)
    }

    /// A listener leaves a socket; a regular file or directory is not its.
    pub(super) fn is_leftover_kind(metadata: &fs::Metadata) -> bool {
        use std::os::unix::fs::FileTypeExt;
        metadata.file_type().is_socket()
    }

    pub(super) fn set_read_timeout(_: &Shared, _: Option<Duration>) -> io::Result<()> {
        // Applied by `read`, which waits for the socket to be readable before
        // it reads. `SO_RCVTIMEO` would do the same, but macOS refuses to set
        // it (`EINVAL`) once the peer has closed, even while bytes the peer
        // sent are still waiting to be read.
        Ok(())
    }

    pub(super) fn set_write_timeout(shared: &Shared, timeout: Option<Duration>) -> io::Result<()> {
        shared.raw.set_send_timeout(timeout)
    }

    /// A socket timeout surfaces as `WouldBlock`; callers see `TimedOut`.
    fn timed_out(result: io::Result<usize>, timeout: Option<Duration>) -> io::Result<usize> {
        match result {
            Err(error)
                if timeout.is_some()
                    && matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
            {
                Err(io::ErrorKind::TimedOut.into())
            }
            other => other,
        }
    }

    /// Returns once a read would not block: data, end of stream or an error,
    /// which the read that follows reports. `TimedOut` after `timeout`.
    fn wait_readable(shared: &Shared, timeout: Duration) -> io::Result<()> {
        let RawStream::UdSocket(socket) = &shared.raw;
        let descriptor = socket.inner().as_raw_fd();
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            // Rounded up, so the wait is never shorter than asked.
            let millis = remaining.as_micros().div_ceil(1000).min(i32::MAX as u128) as libc::c_int;
            let mut poll = libc::pollfd {
                fd: descriptor,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: `poll` is a live local and the descriptor stays owned
            // by the stream for the whole call.
            let ready = unsafe { libc::poll(&mut poll, 1, millis) };
            if ready > 0 {
                return Ok(());
            }
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
    }

    pub(super) fn read(
        shared: &Shared,
        timeout: Option<Duration>,
        buffer: &mut [u8],
    ) -> io::Result<usize> {
        if let Some(timeout) = timeout {
            wait_readable(shared, timeout)?;
        }
        let mut raw = &shared.raw;
        raw.read(buffer)
    }

    pub(super) fn write(
        shared: &Shared,
        timeout: Option<Duration>,
        buffer: &[u8],
    ) -> io::Result<usize> {
        let mut raw = &shared.raw;
        timed_out(raw.write(buffer), timeout)
    }

    pub(super) fn shutdown(shared: &Shared) {
        let RawStream::UdSocket(socket) = &shared.raw;
        let _ = socket.inner().shutdown(Shutdown::Both);
    }

    pub(super) fn peer_pid(shared: &Shared) -> io::Result<u32> {
        let RawStream::UdSocket(socket) = &shared.raw;
        peer_pid_of_fd(socket.inner())
    }

    pub(super) fn pair() -> io::Result<(LocalStream, LocalStream)> {
        let (first, second) = std::os::unix::net::UnixStream::pair()?;
        Ok((
            LocalStream::from_raw(RawStream::UdSocket(first.into())),
            LocalStream::from_raw(RawStream::UdSocket(second.into())),
        ))
    }

    /// A Unix socket queues a connection until it is accepted, a dropped one
    /// included, so `accept` only has to be free to stop: it waits on the
    /// listener and on a wake socket together, and the listener never blocks.
    pub(super) struct Listener {
        raw: RawListener,
        wake: UnixStream,
        closing: Arc<Closing>,
    }

    pub(super) struct Closing {
        closed: AtomicBool,
        signal: UnixStream,
    }

    impl Closing {
        pub(super) fn close(&self) {
            self.closed.store(true, Ordering::SeqCst);
            // The wake end then reads end of stream, which `poll` reports.
            let _ = self.signal.shutdown(Shutdown::Both);
        }
    }

    impl Listener {
        pub(super) fn bind(path: &Path) -> io::Result<Self> {
            let raw = listen(path)?;
            let (wake, signal) = UnixStream::pair()?;
            Ok(Self {
                raw,
                wake,
                closing: Arc::new(Closing {
                    closed: AtomicBool::new(false),
                    signal,
                }),
            })
        }

        pub(super) fn closing(&self) -> Arc<Closing> {
            Arc::clone(&self.closing)
        }

        pub(super) fn accept(&self) -> io::Result<RawStream> {
            let RawListener::UdSocket(listener) = &self.raw;
            loop {
                if self.closing.closed.load(Ordering::SeqCst) {
                    return Err(listener_closed());
                }
                let mut polls = [
                    libc::pollfd {
                        fd: listener.as_fd().as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd {
                        fd: self.wake.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                ];
                // SAFETY: `polls` is a live local array of two entries and
                // both descriptors stay open for the whole call.
                let ready = unsafe { libc::poll(polls.as_mut_ptr(), 2, -1) };
                if ready < 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(error);
                }
                if polls[1].revents != 0 {
                    return Err(listener_closed());
                }
                match self.raw.accept() {
                    Ok(stream) => {
                        // macOS hands out a nonblocking listener's
                        // connections nonblocking too.
                        stream.set_nonblocking(false)?;
                        return Ok(stream);
                    }
                    // Another thread took it between the poll and here.
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }

    fn listen(path: &Path) -> io::Result<RawListener> {
        let create = |with_mode: bool| {
            let mut options = ListenerOptions::new()
                .name(endpoint_name(path)?)
                .nonblocking(ListenerNonblockingMode::Accept)
                .reclaim_name(true);
            if with_mode {
                // Applied before bind, so the socket is never visible with
                // looser permissions.
                options = options.mode(0o600);
            }
            options.create_sync()
        };
        match create(true) {
            Ok(listener) => Ok(listener),
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                // The system cannot fix the mode before bind. Tighten it
                // right after; the parent folder is the account's own.
                use std::os::unix::fs::PermissionsExt;
                let listener = create(false)?;
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
                Ok(listener)
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Mutex, mpsc};
    use std::time::Instant;

    use interprocess::os::windows::security_descriptor::{
        AsSecurityDescriptor, SecurityDescriptor,
    };
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_NO_DATA,
        ERROR_OPERATION_ABORTED, ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, HANDLE,
        INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FlushFileBuffers, PIPE_ACCESS_DUPLEX,
        ReadFile, WriteFile,
    };
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
        GetNamedPipeServerProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, PeekNamedPipe,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects,
    };

    use super::*;

    /// Connects with a bounded wait: the default of `connect` waits for ever
    /// for a pipe whose instances are all busy, which is how Herdr's own
    /// probe avoids it too.
    pub(super) fn connect(path: &Path) -> io::Result<RawStream> {
        use interprocess::ConnectWaitMode;
        use interprocess::os::windows::named_pipe::{DuplexPipeStream, pipe_mode::Bytes};

        let name = pipe_name(path);
        let pipe = DuplexPipeStream::<Bytes>::connect_by_path_with_wait_mode(
            name.as_str(),
            ConnectWaitMode::Timeout(CONNECT_BOUND),
        )
        .map_err(|error| match error.kind() {
            // The wait mode documents `TimedOut` and reports `WouldBlock`.
            io::ErrorKind::WouldBlock => io::Error::from(io::ErrorKind::TimedOut),
            _ => error,
        })?;
        let handle = OwnedHandle::try_from(pipe)
            .map_err(|_| io::Error::other("a fresh pipe stream is not split"))?;
        // A pipe name is not inside the folder its path names, so the folder's
        // access list does not keep another account from creating it first.
        if !crate::fs::private::handle_owned_by_current_user(&handle)? {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the listener at {} belongs to another account",
                    path.display()
                ),
            ));
        }
        Ok(Pipe::new(handle, false))
    }

    /// One connected end of a pipe.
    ///
    /// Not `interprocess`'s stream: the only way into it from a handle
    /// reopens the handle (`ReOpenFile`), and a reopen relative to a pipe
    /// handle is a new client connect to the same pipe. With one instance
    /// listening, as `interprocess`'s own listener keeps, that connect fails
    /// and the original is kept; with several, it takes another instance and
    /// the original end is closed, so the peer's next write fails with "the
    /// pipe is being closed" and the instance taken waits for ever.
    pub(super) struct Pipe {
        handle: OwnedHandle,
        /// The listener's end, which asks for the client's pid.
        server: bool,
        /// Whether anything was written, which the peer may not have read.
        written: AtomicBool,
    }

    impl Pipe {
        fn new(handle: OwnedHandle, server: bool) -> Self {
            Self {
                handle,
                server,
                written: AtomicBool::new(false),
            }
        }

        /// One read or write, overlapped as the handle was opened, waited for
        /// on this thread's own event so a read and a write on two threads
        /// never take each other's completion.
        fn io(&self, start: impl FnOnce(HANDLE, *mut OVERLAPPED) -> i32) -> io::Result<usize> {
            thread_local! {
                static EVENT: std::cell::OnceCell<OwnedHandle> = const { std::cell::OnceCell::new() };
            }
            EVENT.with(|cell| {
                let event = match cell.get() {
                    Some(event) => event,
                    None => {
                        let made = event()?;
                        cell.get_or_init(|| made)
                    }
                };
                let handle = self.handle.as_raw_handle();
                // SAFETY: an all-zero OVERLAPPED is valid; only the event is set.
                let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
                overlapped.hEvent = event.as_raw_handle();
                if start(handle, &mut overlapped) == 0 {
                    let code = last_error_code();
                    if code != ERROR_IO_PENDING {
                        return Err(os_error(code));
                    }
                }
                let mut transferred = 0_u32;
                // SAFETY: the operation started on this OVERLAPPED, which
                // stays on this frame until the wait below has seen it end.
                if unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, 1) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(transferred as usize)
            })
        }

        fn peer_pid(&self) -> io::Result<u32> {
            let mut pid = 0_u32;
            let handle = self.handle.as_raw_handle();
            // SAFETY: the handle is open while `self` is borrowed and the out
            // pointer refers to a live local.
            let answered = unsafe {
                if self.server {
                    GetNamedPipeClientProcessId(handle, &mut pid)
                } else {
                    GetNamedPipeServerProcessId(handle, &mut pid)
                }
            };
            if answered == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(pid)
        }
    }

    /// Closing an end throws away what it wrote and the peer has not read
    /// yet, so an end that wrote is kept open by a thread of its own until
    /// the peer has read it all or gone (`FlushFileBuffers`), as
    /// `interprocess` does. The answer a listener writes just before it drops
    /// the stream is how the pane bootstrap replies.
    impl Drop for Pipe {
        fn drop(&mut self) {
            if !*self.written.get_mut() {
                return;
            }
            let Ok(kept) = self.handle.try_clone() else {
                return;
            };
            let _ = std::thread::Builder::new()
                .name("hide-ipc-linger".to_owned())
                .spawn(move || {
                    // SAFETY: the handle is open for the whole call.
                    unsafe { FlushFileBuffers(kept.as_raw_handle()) };
                });
        }
    }

    impl Read for &Pipe {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
            let data = buffer.as_mut_ptr();
            // SAFETY: the buffer outlives the operation, which `io` waits for.
            self.io(|handle, overlapped| unsafe {
                ReadFile(handle, data, length, std::ptr::null_mut(), overlapped)
            })
        }
    }

    impl Write for &Pipe {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
            let data = buffer.as_ptr();
            // SAFETY: the buffer outlives the operation, which `io` waits for.
            let written = self.io(|handle, overlapped| unsafe {
                WriteFile(handle, data, length, std::ptr::null_mut(), overlapped)
            })?;
            self.written.store(true, Ordering::Relaxed);
            Ok(written)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// The marker file is the only thing a listener leaves on Windows.
    pub(super) fn is_leftover_kind(metadata: &fs::Metadata) -> bool {
        metadata.file_type().is_file()
    }

    /// The owner of the pipe and nobody else; inherited rights are cut off.
    const PRIVATE_SDDL: &str = "D:P(A;;GA;;;OW)";
    /// Waits between looks at a pipe that has no data yet, growing to a cap.
    const FIRST_PAUSE: Duration = Duration::from_millis(1);
    const LONGEST_PAUSE: Duration = Duration::from_millis(4);
    /// How long `shutdown` keeps cancelling a read that is still starting.
    const SHUTDOWN_ATTEMPTS: u32 = 200;

    /// `cancelled` ends the stream; `blocked` counts reads parked inside the
    /// system call, so a `shutdown` that raced one cancels again until it
    /// has left.
    #[derive(Default)]
    pub(super) struct Stop {
        cancelled: AtomicBool,
        blocked: std::sync::atomic::AtomicUsize,
    }

    pub(super) fn raw_handle(raw: &RawStream) -> *mut c_void {
        raw.handle.as_raw_handle()
    }

    pub(super) fn set_read_timeout(_: &Shared, _: Option<Duration>) -> io::Result<()> {
        // Applied by `read`, which looks at the pipe before it reads.
        Ok(())
    }

    pub(super) fn set_write_timeout(_: &Shared, timeout: Option<Duration>) -> io::Result<()> {
        match timeout {
            None => Ok(()),
            Some(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "a Windows named pipe has no write timeout",
            )),
        }
    }

    fn peer_gone(error: &io::Error) -> bool {
        // BROKEN_PIPE, NO_DATA, PIPE_NOT_CONNECTED
        error.kind() == io::ErrorKind::BrokenPipe
            || matches!(error.raw_os_error(), Some(109 | 232 | 233))
    }

    /// Bytes waiting in the pipe, or `None` once the peer is gone.
    fn available(raw: &RawStream) -> io::Result<Option<u32>> {
        let mut waiting = 0_u32;
        // SAFETY: the handle is open for as long as `raw` is borrowed and the
        // out pointer refers to a live local; the buffers are null with
        // length zero, which is how a peek asks for the byte count only.
        let peeked = unsafe {
            PeekNamedPipe(
                raw_handle(raw),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut waiting,
                std::ptr::null_mut(),
            )
        };
        if peeked != 0 {
            return Ok(Some(waiting));
        }
        let error = io::Error::last_os_error();
        if peer_gone(&error) {
            Ok(None)
        } else {
            Err(error)
        }
    }

    fn settle(stop: &Stop, result: io::Result<usize>) -> io::Result<usize> {
        match result {
            Err(error)
                if stop.cancelled.load(Ordering::SeqCst)
                    && error.raw_os_error() == Some(ERROR_OPERATION_ABORTED as i32) =>
            {
                Ok(0)
            }
            Err(error) if peer_gone(&error) => Ok(0),
            other => other,
        }
    }

    pub(super) fn read(
        shared: &Shared,
        timeout: Option<Duration>,
        buffer: &mut [u8],
    ) -> io::Result<usize> {
        let stop = &shared.stop;
        let mut raw = &shared.raw;
        let Some(timeout) = timeout else {
            stop.blocked.fetch_add(1, Ordering::SeqCst);
            let result = if stop.cancelled.load(Ordering::SeqCst) {
                Ok(0)
            } else {
                raw.read(buffer)
            };
            stop.blocked.fetch_sub(1, Ordering::SeqCst);
            return settle(stop, result);
        };
        let deadline = Instant::now() + timeout;
        let mut pause = FIRST_PAUSE;
        loop {
            if stop.cancelled.load(Ordering::SeqCst) {
                return Ok(0);
            }
            match available(raw)? {
                None => return Ok(0),
                Some(0) => {}
                Some(_) => return settle(stop, raw.read(buffer)),
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            std::thread::sleep(pause.min(remaining));
            pause = (pause * 2).min(LONGEST_PAUSE);
        }
    }

    pub(super) fn write(shared: &Shared, _: Option<Duration>, buffer: &[u8]) -> io::Result<usize> {
        if shared.stop.cancelled.load(Ordering::SeqCst) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let mut raw = &shared.raw;
        raw.write(buffer).map_err(|error| {
            if shared.stop.cancelled.load(Ordering::SeqCst) {
                io::ErrorKind::BrokenPipe.into()
            } else {
                error
            }
        })
    }

    pub(super) fn shutdown(shared: &Shared) {
        let stop = &shared.stop;
        stop.cancelled.store(true, Ordering::SeqCst);
        let handle = raw_handle(&shared.raw);
        for _ in 0..SHUTDOWN_ATTEMPTS {
            // SAFETY: the handle is open while `shared` is borrowed; a null
            // overlapped pointer cancels every pending request on it.
            let _ = unsafe { CancelIoEx(handle, std::ptr::null()) };
            if stop.blocked.load(Ordering::SeqCst) == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub(super) fn peer_pid(shared: &Shared) -> io::Result<u32> {
        shared.raw.peer_pid()
    }

    /// A pipe has no unnamed pair, so the two ends meet on a private name that
    /// goes away with the listener.
    pub(super) fn pair() -> io::Result<(LocalStream, LocalStream)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hide-pair-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let listener = LocalListener::bind(&path)?;
        let client = LocalStream::connect(&path)?;
        let server = listener.accept()?;
        Ok((client, server))
    }

    /// Pipe instances kept listening at once. A client takes an instance when
    /// it connects, and holds it until the instance is served even when it has
    /// already left, so with one instance (what `interprocess`'s listener
    /// keeps) a client that came and went blocked every connect behind it
    /// (#315). With several, the others still answer while one is replaced.
    const INSTANCES: usize = 4;
    /// Connections served but not yet accepted, as a socket's backlog; past
    /// it the accept thread waits and the instances fill up.
    const QUEUE: usize = 64;
    /// What `interprocess` asked for; the system grows the buffer as needed.
    const BUFFER_HINT: u32 = 512;

    fn os_error(code: u32) -> io::Error {
        io::Error::from_raw_os_error(code as i32)
    }

    fn last_error_code() -> u32 {
        io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32
    }

    fn owned(raw: HANDLE) -> OwnedHandle {
        // SAFETY: callers pass a handle they just created and own alone.
        unsafe { OwnedHandle::from_raw_handle(raw) }
    }

    /// One pipe name and the access list every instance of it gets.
    struct Endpoint {
        name: widestring::U16CString,
        security: SecurityDescriptor,
    }

    impl Endpoint {
        fn instance(&self, first: bool) -> io::Result<OwnedHandle> {
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.security.as_sd().cast_mut(),
                bInheritHandle: 0,
            };
            let mut open = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
            if first {
                // Another live listener on the name refuses this one.
                open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
            }
            // SAFETY: the name is NUL-terminated and the attributes and the
            // descriptor they point at outlive the call.
            let pipe = unsafe {
                CreateNamedPipeW(
                    self.name.as_ptr(),
                    open,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    PIPE_UNLIMITED_INSTANCES,
                    BUFFER_HINT,
                    BUFFER_HINT,
                    0,
                    &attributes,
                )
            };
            if pipe == INVALID_HANDLE_VALUE {
                let error = io::Error::last_os_error();
                if first && error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "another listener owns this pipe name",
                    ));
                }
                return Err(error);
            }
            Ok(owned(pipe))
        }
    }

    fn event() -> io::Result<OwnedHandle> {
        // SAFETY: no attributes and no name; a manual-reset event, unset.
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(owned(event))
    }

    pub(super) struct Closing {
        closed: AtomicBool,
        stop: OwnedHandle,
    }

    impl Closing {
        pub(super) fn close(&self) {
            self.closed.store(true, Ordering::SeqCst);
            // SAFETY: the event is open for as long as `self` is borrowed.
            unsafe { SetEvent(self.stop.as_raw_handle()) };
        }
    }

    /// The pipe instances are served by a thread of the listener's own, which
    /// clears an instance a client left and queues one a client holds, so
    /// connects never wait on whoever calls `accept`.
    pub(super) struct Listener {
        accepted: Mutex<Option<mpsc::Receiver<io::Result<OwnedHandle>>>>,
        closing: Arc<Closing>,
        serving: Option<std::thread::JoinHandle<()>>,
    }

    impl Listener {
        pub(super) fn bind(path: &Path) -> io::Result<Self> {
            let sddl = widestring::U16CString::from_str(PRIVATE_SDDL)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
            let name = widestring::U16CString::from_str(pipe_name(path))
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
            let endpoint = Endpoint {
                name,
                security: SecurityDescriptor::deserialize(&sddl)?,
            };
            let mut instances = Vec::with_capacity(INSTANCES);
            for index in 0..INSTANCES {
                instances.push(Instance {
                    pipe: endpoint.instance(index == 0)?,
                    event: event()?,
                });
            }
            let closing = Arc::new(Closing {
                closed: AtomicBool::new(false),
                stop: event()?,
            });
            let (queue, accepted) = mpsc::sync_channel(QUEUE);
            let stop = closing.stop.try_clone()?;
            let serving = std::thread::Builder::new()
                .name("hide-ipc-accept".to_owned())
                .spawn(move || serve(endpoint, instances, stop, queue))?;
            Ok(Self {
                accepted: Mutex::new(Some(accepted)),
                closing,
                serving: Some(serving),
            })
        }

        pub(super) fn closing(&self) -> Arc<Closing> {
            Arc::clone(&self.closing)
        }

        pub(super) fn accept(&self) -> io::Result<RawStream> {
            if self.closing.closed.load(Ordering::SeqCst) {
                return Err(listener_closed());
            }
            let accepted = self
                .accepted
                .lock()
                .map_err(|_| io::Error::other("an accept panicked"))?;
            let Some(accepted) = accepted.as_ref() else {
                return Err(listener_closed());
            };
            match accepted.recv() {
                _ if self.closing.closed.load(Ordering::SeqCst) => Err(listener_closed()),
                Ok(Ok(handle)) => Ok(Pipe::new(handle, true)),
                Ok(Err(error)) => Err(error),
                // The thread ends only when closed or when every instance
                // failed, and it queued each failure first.
                Err(_) => Err(io::Error::other("the listener stopped serving")),
            }
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            self.closing.close();
            // A thread waiting for room in the queue gives up once the queue
            // is gone.
            self.accepted
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(serving) = self.serving.take() {
                let _ = serving.join();
            }
        }
    }

    struct Instance {
        pipe: OwnedHandle,
        event: OwnedHandle,
    }

    /// One instance's connect in flight. The `OVERLAPPED` is boxed because
    /// the system writes to it until the connect completes or is cancelled.
    struct Slot {
        instance: Option<Instance>,
        overlapped: Box<OVERLAPPED>,
        pending: bool,
    }

    enum Outcome {
        Pending,
        /// A client holds the instance; it may already have left.
        Connected,
        /// A client connected and left before the instance was served.
        Gone,
    }

    impl Slot {
        fn pipe(&self) -> HANDLE {
            self.instance
                .as_ref()
                .map_or(std::ptr::null_mut(), |instance| {
                    instance.pipe.as_raw_handle()
                })
        }

        /// Starts listening on the instance, answering at once when a client
        /// is already there.
        fn start(&mut self) -> io::Result<Outcome> {
            let Some(instance) = self.instance.as_ref() else {
                return Ok(Outcome::Pending);
            };
            let event = instance.event.as_raw_handle();
            // SAFETY: the event is open while `instance` is borrowed.
            unsafe { ResetEvent(event) };
            // SAFETY: an all-zero OVERLAPPED is valid; only the event is set.
            *self.overlapped = unsafe { std::mem::zeroed() };
            self.overlapped.hEvent = event;
            // SAFETY: the pipe is open and the boxed OVERLAPPED stays put
            // until the connect completes or is cancelled in `cancel`.
            if unsafe { ConnectNamedPipe(self.pipe(), &mut *self.overlapped) } != 0 {
                return Ok(Outcome::Connected);
            }
            match last_error_code() {
                ERROR_IO_PENDING => {
                    self.pending = true;
                    Ok(Outcome::Pending)
                }
                ERROR_PIPE_CONNECTED => Ok(Outcome::Connected),
                ERROR_NO_DATA => Ok(Outcome::Gone),
                code => Err(os_error(code)),
            }
        }

        /// How the connect that signalled the event ended.
        fn finish(&mut self) -> io::Result<Outcome> {
            self.pending = false;
            let mut transferred = 0_u32;
            // SAFETY: the connect this OVERLAPPED started has completed.
            if unsafe { GetOverlappedResult(self.pipe(), &*self.overlapped, &mut transferred, 0) }
                != 0
            {
                return Ok(Outcome::Connected);
            }
            match last_error_code() {
                ERROR_NO_DATA | ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED => Ok(Outcome::Gone),
                code => Err(os_error(code)),
            }
        }

        /// Frees the instance a client left, so it can listen again.
        fn clear(&mut self) -> io::Result<()> {
            // SAFETY: the pipe is open; no connect is in flight on it.
            if unsafe { DisconnectNamedPipe(self.pipe()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        /// Ends a connect in flight and waits until the system lets go of the
        /// OVERLAPPED.
        fn cancel(&mut self) {
            if !self.pending {
                return;
            }
            let mut transferred = 0_u32;
            // SAFETY: the pipe is open and this OVERLAPPED is its connect's;
            // waiting for the result keeps the box alive until it is done.
            unsafe {
                CancelIoEx(self.pipe(), &*self.overlapped);
                GetOverlappedResult(self.pipe(), &*self.overlapped, &mut transferred, 1);
            }
            self.pending = false;
        }
    }

    /// The accept thread: keeps every instance listening, hands a connected
    /// one to the queue and puts a fresh instance in its place, and clears
    /// one a client left. Ends when stopped, when the queue is dropped, or
    /// when no instance is left; a failure is queued for `accept` to report.
    fn serve(
        endpoint: Endpoint,
        instances: Vec<Instance>,
        stop: OwnedHandle,
        queue: mpsc::SyncSender<io::Result<OwnedHandle>>,
    ) {
        let mut slots: Vec<Slot> = instances
            .into_iter()
            .map(|instance| Slot {
                instance: Some(instance),
                // SAFETY: an all-zero OVERLAPPED is valid.
                overlapped: Box::new(unsafe { std::mem::zeroed() }),
                pending: false,
            })
            .collect();
        'serving: loop {
            for slot in &mut slots {
                // A client may already be there, or may come and go again
                // while the instance is being cleared.
                while slot.instance.is_some() && !slot.pending {
                    let outcome = slot.start();
                    if !act_on(&endpoint, slot, outcome, &queue) {
                        break 'serving;
                    }
                }
            }
            slots.retain(|slot| slot.instance.is_some());
            if slots.is_empty() {
                break;
            }
            let mut waits: Vec<HANDLE> = slots
                .iter()
                .filter_map(|slot| slot.instance.as_ref())
                .map(|instance| instance.event.as_raw_handle())
                .collect();
            waits.push(stop.as_raw_handle());
            // SAFETY: every handle in `waits` is open for the whole wait.
            let woke =
                unsafe { WaitForMultipleObjects(waits.len() as u32, waits.as_ptr(), 0, INFINITE) };
            let index = woke.wrapping_sub(WAIT_OBJECT_0) as usize;
            if index == slots.len() {
                break;
            }
            if index > slots.len() {
                let _ = queue.send(Err(io::Error::last_os_error()));
                break;
            }
            let outcome = slots[index].finish();
            if !act_on(&endpoint, &mut slots[index], outcome, &queue) {
                break;
            }
        }
        for slot in &mut slots {
            slot.cancel();
        }
    }

    /// Acts on how an instance's connect ended. `false` when the thread has
    /// to stop because nobody takes from the queue any more.
    fn act_on(
        endpoint: &Endpoint,
        slot: &mut Slot,
        outcome: io::Result<Outcome>,
        queue: &mpsc::SyncSender<io::Result<OwnedHandle>>,
    ) -> bool {
        match outcome {
            Ok(Outcome::Pending) => true,
            Ok(Outcome::Gone) => match slot.clear() {
                Ok(()) => true,
                // An instance that cannot be cleared would answer "gone" to
                // every connect after it; it is given up like a failed one.
                Err(error) => {
                    slot.instance = None;
                    queue.send(Err(error)).is_ok()
                }
            },
            Ok(Outcome::Connected) => {
                let Some(instance) = slot.instance.as_mut() else {
                    return true;
                };
                // A fresh instance takes the served one's place before the
                // served one is queued, so the slot listens again at once.
                match endpoint.instance(false) {
                    Ok(fresh) => {
                        let served = std::mem::replace(&mut instance.pipe, fresh);
                        queue.send(Ok(served)).is_ok()
                    }
                    Err(error) => {
                        let served = slot.instance.take().map(|instance| instance.pipe);
                        served.is_none_or(|pipe| queue.send(Ok(pipe)).is_ok())
                            && queue.send(Err(error)).is_ok()
                    }
                }
            }
            Err(error) => {
                // This instance cannot listen any more; the others go on.
                slot.instance = None;
                queue.send(Err(error)).is_ok()
            }
        }
    }

    /// Records that this listener, and not another, owns `path`.
    pub(super) fn write_marker(path: &Path) -> io::Result<String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let marker = format!("{}:{now}", std::process::id());
        fs::write(path, &marker)?;
        Ok(marker)
    }
}
