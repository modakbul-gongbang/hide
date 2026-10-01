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
//!   that is blocked, with `Ok(0)`, and every later read and write fails.
//! - [`LocalListener::bind`] fails with `AddrInUse` while another listener
//!   answers at the path, and replaces what a dead one left behind.
//! - Only the account that bound the listener can connect to it.
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
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{
    Listener as RawListener, ListenerOptions, Name, Stream as RawStream,
};

/// What a listener's path may hold when nobody listens: connecting to it is
/// refused, finds nothing, or times out.
fn leftover_connect_error(kind: io::ErrorKind) -> bool {
    matches!(
        kind,
        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound | io::ErrorKind::TimedOut
    ) || (cfg!(windows) && kind == io::ErrorKind::WouldBlock)
}

#[cfg(unix)]
fn endpoint_name(path: &Path) -> io::Result<Name<'_>> {
    path.to_fs_name::<GenericFilePath>()
}

#[cfg(windows)]
fn endpoint_name(path: &Path) -> io::Result<Name<'_>> {
    path.to_string_lossy()
        .into_owned()
        .to_ns_name::<GenericNamespaced>()
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
    /// when nobody answers there.
    pub fn connect(path: &Path) -> io::Result<Self> {
        RawStream::connect(endpoint_name(path)?).map(Self::from_raw)
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
    raw: RawListener,
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
        let raw = sys::listen(path)?;
        #[cfg(windows)]
        let marker = sys::write_marker(path)?;
        Ok(Self {
            raw,
            path: path.to_path_buf(),
            #[cfg(windows)]
            marker,
        })
    }

    /// Waits for the next connection.
    pub fn accept(&self) -> io::Result<LocalStream> {
        self.raw.accept().map(LocalStream::from_raw)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
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

/// Removes what a dead listener left at `path`, and refuses a live one.
fn clear_leftover(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path).is_err() {
        return Ok(());
    }
    match LocalStream::connect(path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("a listener already answers at {}", path.display()),
        )),
        Err(error) if leftover_connect_error(error.kind()) => match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
mod sys {
    use std::net::Shutdown;
    use std::os::fd::AsRawFd;
    use std::time::Instant;

    use interprocess::os::unix::local_socket::ListenerOptionsExt as _;

    use super::*;

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

    #[cfg(target_os = "macos")]
    pub(super) fn peer_pid(shared: &Shared) -> io::Result<u32> {
        let RawStream::UdSocket(socket) = &shared.raw;
        let mut pid: libc::pid_t = 0;
        let mut size = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
        // SAFETY: the output pointers refer to initialized stack values and
        // the descriptor stays owned by the stream for the whole call.
        let result = unsafe {
            libc::getsockopt(
                socket.inner().as_raw_fd(),
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

    #[cfg(not(target_os = "macos"))]
    pub(super) fn peer_pid(shared: &Shared) -> io::Result<u32> {
        shared
            .raw
            .peer_creds()?
            .pid()
            .and_then(|pid| u32::try_from(pid).ok())
            .ok_or_else(|| io::Error::from(io::ErrorKind::Unsupported))
    }

    pub(super) fn pair() -> io::Result<(LocalStream, LocalStream)> {
        let (first, second) = std::os::unix::net::UnixStream::pair()?;
        Ok((
            LocalStream::from_raw(RawStream::UdSocket(first.into())),
            LocalStream::from_raw(RawStream::UdSocket(second.into())),
        ))
    }

    pub(super) fn listen(path: &Path) -> io::Result<RawListener> {
        let create = |with_mode: bool| {
            let mut options = ListenerOptions::new()
                .name(endpoint_name(path)?)
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
    use std::os::windows::io::{AsHandle, AsRawHandle};
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;

    use interprocess::os::windows::local_socket::ListenerOptionsExt as _;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use windows_sys::Win32::Foundation::ERROR_OPERATION_ABORTED;
    use windows_sys::Win32::System::IO::CancelIoEx;
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;

    use super::*;

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
        let RawStream::NamedPipe(pipe) = raw;
        pipe.as_handle().as_raw_handle()
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
        shared
            .raw
            .peer_creds()?
            .pid()
            .ok_or_else(|| io::Error::from(io::ErrorKind::Unsupported))
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

    pub(super) fn listen(path: &Path) -> io::Result<RawListener> {
        let sddl = widestring::U16CString::from_str(PRIVATE_SDDL)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        ListenerOptions::new()
            .name(endpoint_name(path)?)
            .reclaim_name(false)
            .security_descriptor(SecurityDescriptor::deserialize(&sddl)?)
            .create_sync()
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
