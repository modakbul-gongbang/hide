use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, ThreadId};

use crate::model::CoreOptions;
use crate::runtime::{Runtime, validate_options};
use crate::{environment, live};

#[repr(C)]
pub struct HerdrBytes {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl HerdrBytes {
    fn empty() -> Self {
        Self {
            ptr: ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    fn from_vec(mut bytes: Vec<u8>) -> Self {
        let result = Self {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            cap: bytes.capacity(),
        };
        std::mem::forget(bytes);
        result
    }
}

#[derive(Clone, Copy)]
struct CCallback {
    callback: extern "C" fn(*mut c_void),
    context: *mut c_void,
}

unsafe impl Send for CCallback {}
unsafe impl Sync for CCallback {}

enum NotifyTarget {
    C(CCallback),
    Rust(Arc<dyn Fn() + Send + Sync>),
}

impl Clone for NotifyTarget {
    fn clone(&self) -> Self {
        match self {
            Self::C(callback) => Self::C(*callback),
            Self::Rust(callback) => Self::Rust(Arc::clone(callback)),
        }
    }
}

/// Thread-safe handle that fires the registered change callback. Cloned into
/// live worker threads so PTY and session-sync output can wake the Swift shell.
///
/// Announcements coalesce. The shell answers one by reading the whole
/// snapshot, so every change between an announcement and the read that
/// answers it is already carried by that read; announcing each one separately
/// bought the main thread one hop through the run loop and one turn waiting
/// on the runtime mutex per PTY chunk.
#[derive(Clone)]
pub struct ChangeNotifier {
    registration: Arc<Mutex<Option<NotifyTarget>>>,
    /// True from an announcement until the read that answers it begins.
    announced: Arc<AtomicBool>,
}

impl ChangeNotifier {
    fn new() -> Self {
        Self {
            registration: Arc::new(Mutex::new(None)),
            announced: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn notify(&self) {
        let registration = lock_recover(&self.registration).clone();
        // Latching with nobody listening would swallow the first real
        // announcement, so an unregistered notifier stays silent and unlatched.
        let Some(registration) = registration else {
            return;
        };
        if self.announced.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = catch_unwind(AssertUnwindSafe(|| match registration {
            NotifyTarget::C(target) => (target.callback)(target.context),
            NotifyTarget::Rust(callback) => callback(),
        }));
    }

    /// Called before the reader takes the runtime lock, never after.
    ///
    /// Clearing first means a change that lands while the delta is being taken
    /// announces itself again; the reader may then run once for nothing, which
    /// costs a read. Clearing afterwards would read that announcement as
    /// already delivered and leave the change on screen-invisible state until
    /// something else happened to notify.
    fn clear_announcement(&self) {
        self.announced.store(false, Ordering::Release);
    }

    fn set_callback(&self, registration: Option<NotifyTarget>) {
        *lock_recover(&self.registration) = registration;
    }

    #[cfg(test)]
    pub(crate) fn noop() -> Self {
        Self::new()
    }
}

#[repr(C)]
pub struct HerdrCore {
    _terminal_maintenance: Option<crate::terminal_recovery::Maintenance>,
    _session_sync: Option<crate::session_sync::SessionSyncHandle>,
    runtime: Arc<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    owner_thread: ThreadId,
}

impl Drop for HerdrCore {
    fn drop(&mut self) {
        // Reserve cancellation before any coordinator shutdown can wait: a
        // completing attachment must not enqueue input during destruction.
        let attachment_worker = { lock_recover(&self.runtime).take_attachment_worker() };
        self._terminal_maintenance.take();
        self._session_sync.take();
        // Taken under the lock, joined outside it: a coordinator's last act is
        // to lock the runtime, so a join under the lock never returns.
        let remote_syncs = { lock_recover(&self.runtime).take_remote_syncs() };
        drop(remote_syncs);
        if let Some(worker) = attachment_worker {
            let _ = worker.join();
        }
        let worker = { lock_recover(&self.runtime).take_state_save_worker() };
        if let Some(worker) = worker
            && worker.join().is_err()
        {
            crate::diagnostic!(
                serde_json::json!({"component":"ui_state", "kind":"save.join_failed"})
            );
        }
    }
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn core_ref<'a>(core: *mut HerdrCore) -> Option<&'a HerdrCore> {
    if core.is_null() {
        None
    } else {
        Some(unsafe { &*core })
    }
}

unsafe fn drop_core(core: *mut HerdrCore) {
    unsafe {
        drop(Box::from_raw(core));
    }
}

fn input_bytes<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if ptr.is_null() {
        return (len == 0).then_some(&[]);
    }
    Some(unsafe { slice::from_raw_parts(ptr, len) })
}

fn check_owner_thread(core: &HerdrCore, operation: &str) -> bool {
    if thread::current().id() == core.owner_thread {
        return true;
    }
    lock_recover(&core.runtime).set_error(
        "ffi.wrong_thread",
        format!("{operation} must run on the thread that created herdr-core"),
        false,
    );
    false
}

fn notify_change(core: &HerdrCore) {
    core.notifier.notify();
}

/// Safe Rust handle for the same core the C ABI exposes.
///
/// `create`, `dispatch`, `snapshot_delta`, `on_change`, and `Drop` (destroy)
/// belong to the thread that created the value. Axum workers talk to it
/// through an owner-thread channel in `hided`.
pub type Core = HerdrCore;

impl HerdrCore {
    pub fn create(options: CoreOptions) -> Option<Box<Self>> {
        ignore_sigpipe();
        if validate_options(&options).is_err() {
            return None;
        }
        let mut options = options;
        let environment = environment::read_and_validate();
        let usage_paths = crate::usage::UsagePaths {
            home: environment.home_path.clone(),
            claude_cwd: std::path::Path::new(&options.app_state_path)
                .parent()
                .filter(|directory| !directory.as_os_str().is_empty())
                .map(std::path::Path::to_path_buf),
            codex_home: environment.codex_home.clone(),
        };
        if options.herdr_socket_path.is_some()
            && let Some(path) = environment.herdr_socket_path_override.as_ref()
        {
            options.herdr_socket_path = Some(path.clone());
        }
        #[cfg(not(test))]
        if let Err(error) =
            crate::diagnostics::install(std::path::Path::new(&options.app_state_path))
        {
            std::eprintln!(
                "{}",
                serde_json::json!({"kind": "diagnostics.open_failed", "message": error.to_string()})
            );
        }
        let runtime = Arc::new(Mutex::new(Runtime::new(options.clone(), environment)));
        let notifier = ChangeNotifier::new();
        lock_recover(&runtime).install_worker_context(Arc::downgrade(&runtime), notifier.clone());
        let session_sync = if let Some(socket_path) = options.herdr_socket_path.as_deref() {
            live::install(
                &runtime,
                notifier.clone(),
                socket_path,
                options.herdr_bin_path.as_deref(),
                usage_paths,
            )
        } else {
            None
        };
        if lock_recover(&runtime).connect_registered_devices() {
            notifier.notify();
        }
        let maintenance = match crate::terminal_recovery::Maintenance::spawn(
            Arc::downgrade(&runtime),
            notifier.clone(),
        ) {
            Ok(handle) => Some(handle),
            Err(error) => {
                lock_recover(&runtime).set_error(
                    "terminal.recovery_unavailable",
                    error.to_string(),
                    true,
                );
                None
            }
        };
        Some(Box::new(HerdrCore {
            _terminal_maintenance: maintenance,
            _session_sync: session_sync,
            runtime,
            notifier,
            owner_thread: thread::current().id(),
        }))
    }

    pub fn create_from_json(options_json: &[u8]) -> Option<Box<Self>> {
        let options = serde_json::from_slice::<CoreOptions>(options_json).ok()?;
        Self::create(options)
    }

    pub fn dispatch_bytes(&self, bytes: &[u8]) -> bool {
        if !check_owner_thread(self, "dispatch") {
            notify_change(self);
            return false;
        }
        let (changed, retired_syncs) = {
            let mut runtime = lock_recover(&self.runtime);
            let changed = runtime.dispatch_json(bytes);
            (changed, runtime.take_retired_remote_syncs())
        };
        drop(retired_syncs);
        if changed {
            notify_change(self);
        }
        changed
    }

    /// Rust-only daemon capability handoff. This does not alter the six-call C ABI.
    pub fn set_file_roots(&self, roots: crate::files::FileRoots) {
        if check_owner_thread(self, "set_file_roots") {
            lock_recover(&self.runtime).set_file_roots(roots);
        }
    }

    pub fn snapshot_delta(&self, have_revision: u64, have_terminal_sequence: u64) -> Vec<u8> {
        if !check_owner_thread(self, "snapshot") {
            notify_change(self);
            return Vec::new();
        }
        self.notifier.clear_announcement();
        let payload = {
            let mut runtime = lock_recover(&self.runtime);
            runtime.snapshot_delta_payload(have_revision, have_terminal_sequence)
        };
        crate::runtime::serialize_snapshot_delta(&payload).unwrap_or_default()
    }

    pub fn on_change<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        if !check_owner_thread(self, "on_change") {
            notify_change(self);
            return;
        }
        self.notifier
            .set_callback(Some(NotifyTarget::Rust(Arc::new(callback))));
    }

    pub fn clear_on_change(&self) {
        if !check_owner_thread(self, "on_change") {
            notify_change(self);
            return;
        }
        self.notifier.set_callback(None);
    }
}

/// Makes a write to a closed pipe or socket return `EPIPE` instead of
/// terminating the process.
///
/// A Rust binary does this in its own startup, but this core is a static
/// library inside a Swift host, which leaves SIGPIPE at its default action:
/// terminate, with no crash report. On 2026-09-04 closing a tab exited the app
/// that way: the pane's control child had already left on `terminal_closed`,
/// and the release line the session drop writes to its stdin hit the closed
/// pipe. Every pipe write in this core reports its error to the runtime, so
/// this is what lets those reports happen.
fn ignore_sigpipe() {
    // SAFETY: installing SIG_IGN for SIGPIPE has no handler to race with and
    // no memory to hand over; the call only changes the process signal table.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_create(options_json: *const u8, len: usize) -> *mut HerdrCore {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(bytes) = input_bytes(options_json, len) else {
            return ptr::null_mut();
        };
        HerdrCore::create_from_json(bytes)
            .map(Box::into_raw)
            .unwrap_or(ptr::null_mut())
    }))
    .unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_dispatch(core: *mut HerdrCore, event_json: *const u8, len: usize) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(core) = core_ref(core) else {
            return;
        };
        let Some(bytes) = input_bytes(event_json, len) else {
            lock_recover(&core.runtime).set_error(
                "event.invalid_pointer",
                "Event pointer was null for a non-empty payload",
                false,
            );
            notify_change(core);
            return;
        };
        let _ = core.dispatch_bytes(bytes);
    }));
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_snapshot(
    core: *mut HerdrCore,
    have_revision: u64,
    have_terminal_sequence: u64,
) -> HerdrBytes {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(core) = core_ref(core) else {
            return HerdrBytes::empty();
        };
        let bytes = core.snapshot_delta(have_revision, have_terminal_sequence);
        if bytes.is_empty() {
            HerdrBytes::empty()
        } else {
            HerdrBytes::from_vec(bytes)
        }
    }))
    .unwrap_or_else(|_| HerdrBytes::empty())
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_on_change(
    core: *mut HerdrCore,
    callback: Option<extern "C" fn(*mut c_void)>,
    context: *mut c_void,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(core) = core_ref(core) else {
            return;
        };
        if !check_owner_thread(core, "on_change") {
            notify_change(core);
            return;
        }
        core.notifier.set_callback(
            callback.map(|callback| NotifyTarget::C(CCallback { callback, context })),
        );
    }));
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_free_bytes(bytes: HerdrBytes) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if bytes.ptr.is_null() {
            return;
        }
        unsafe {
            drop(Vec::from_raw_parts(bytes.ptr, bytes.len, bytes.cap));
        }
    }));
}

#[unsafe(no_mangle)]
// C callers own this allocation and cannot express Rust's `unsafe fn` contract.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn herdr_core_destroy(core: *mut HerdrCore) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if core.is_null() {
            return;
        }
        let Some(core_ref) = core_ref(core) else {
            return;
        };
        if !check_owner_thread(core_ref, "destroy") {
            notify_change(core_ref);
            return;
        }
        core_ref.notifier.set_callback(None);
        unsafe {
            drop_core(core);
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A write to a pipe whose reader is gone comes back as an error the
    /// caller sees, instead of ending the process. The test harness already
    /// ignores SIGPIPE for its own process, so the default action is
    /// restored first to make the call under test do the work.
    ///
    /// The loop is not a retry of the assertion: it re-establishes the
    /// precondition. Other tests in this binary spawn fixture processes, and a
    /// child inherits every descriptor open at the instant it forks, so a
    /// child that starts between `pipe` and the close-on-exec flags keeps the
    /// read end open and the pipe stays writable. That is a different
    /// situation from the one under test, not a passing one, so an attempt
    /// that meets it is discarded and a fresh pipe is made. A write that
    /// succeeds is never accepted as a result.
    #[test]
    fn a_write_to_a_closed_pipe_fails_instead_of_terminating_the_process() {
        use std::io::Write;
        use std::os::fd::FromRawFd;

        for _ in 0..16 {
            // SAFETY: restoring the default disposition, creating a pipe and
            // marking its ends close-on-exec are plain libc calls with no
            // memory handed across the boundary.
            let (reader, mut writer) = unsafe {
                libc::signal(libc::SIGPIPE, libc::SIG_DFL);
                let mut fds = [0; 2];
                assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
                for fd in fds {
                    assert_ne!(libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC), -1);
                }
                (
                    std::fs::File::from_raw_fd(fds[0]),
                    std::fs::File::from_raw_fd(fds[1]),
                )
            };
            ignore_sigpipe();
            drop(reader);
            match writer.write_all(b"release\n") {
                // Someone else still holds the read end; try a fresh pipe.
                Ok(()) => continue,
                Err(error) => {
                    assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
                    return;
                }
            }
        }
        panic!(
            "every pipe was inherited by a concurrently spawned fixture, so the write under test never happened"
        );
    }
}
