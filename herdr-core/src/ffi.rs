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
struct CallbackRegistration {
    callback: extern "C" fn(*mut c_void),
    context: *mut c_void,
}

unsafe impl Send for CallbackRegistration {}
unsafe impl Sync for CallbackRegistration {}

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
    registration: Arc<Mutex<Option<CallbackRegistration>>>,
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
        let registration = *lock_recover(&self.registration);
        // Latching with nobody listening would swallow the first real
        // announcement, so an unregistered notifier stays silent and unlatched.
        let Some(registration) = registration else {
            return;
        };
        if self.announced.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = catch_unwind(AssertUnwindSafe(|| {
            (registration.callback)(registration.context);
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

    fn set_callback(&self, registration: Option<CallbackRegistration>) {
        *lock_recover(&self.registration) = registration;
    }

    #[cfg(test)]
    pub(crate) fn noop() -> Self {
        Self::new()
    }
}

#[repr(C)]
pub struct HerdrCore {
    _session_sync: Option<crate::session_sync::SessionSyncHandle>,
    _remote_session_sync: Vec<crate::session_sync::SessionSyncHandle>,
    runtime: Arc<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    owner_thread: ThreadId,
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
        ignore_sigpipe();
        let Some(bytes) = input_bytes(options_json, len) else {
            return ptr::null_mut();
        };
        let Ok(mut options) = serde_json::from_slice::<CoreOptions>(bytes) else {
            return ptr::null_mut();
        };
        if validate_options(&options).is_err() {
            return ptr::null_mut();
        }
        let environment = environment::read_and_validate();
        let home_path = environment.home_path.clone();
        let remote_enabled = environment.remote_enabled;
        if options.herdr_socket_path.is_some()
            && let Some(path) = environment.herdr_socket_path_override.as_ref()
        {
            options.herdr_socket_path = Some(path.clone());
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
                home_path.clone(),
            )
        } else {
            None
        };
        let mut remote_session_sync = Vec::new();
        if remote_enabled {
            for target in &options.remote_targets {
                let result = (|| {
                    let home_path = home_path.as_ref().ok_or_else(|| {
                        "HOME is unavailable, so the SSH config cannot be resolved".to_owned()
                    })?;
                    let alias = crate::remote::SshAlias::from_config_file(
                        &home_path.join(".ssh/config"),
                        &target.ssh_alias,
                    )
                    .map_err(|error| error.to_string())?;
                    let client = Arc::new(
                        crate::remote::RusshRemoteClient::new(alias)
                            .map_err(|error| error.to_string())?,
                    );
                    let connector = client
                        .herdr_api_connector(target.herdr_socket_path.clone())
                        .map_err(|error| error.to_string())?;
                    let connector: Arc<dyn crate::herdr_api::ApiConnector> = Arc::new(connector);
                    lock_recover(&runtime).install_remote_control(
                        crate::live::RemoteControlContext::new(
                            target.id.clone(),
                            Arc::clone(&connector),
                            Arc::downgrade(&runtime),
                            notifier.clone(),
                        ),
                    );
                    lock_recover(&runtime).install_remote_terminal(
                        crate::live::RemoteTerminalContext::new(
                            target.id.clone(),
                            Arc::clone(&client),
                            target.herdr_socket_path.clone(),
                            Arc::downgrade(&runtime),
                            notifier.clone(),
                        ),
                    );
                    lock_recover(&runtime).install_remote_file_transport(
                        target.id.clone(),
                        crate::remote::RusshSftpTransport::new(Arc::clone(&client)),
                    );
                    let context = crate::session_sync::SessionSyncContext::remote(
                        target.id.clone(),
                        target.label.clone(),
                        connector,
                        Arc::downgrade(&runtime),
                        notifier.clone(),
                    );
                    crate::session_sync::spawn(context, None)
                })();
                match result {
                    Ok(handle) => remote_session_sync.push(handle),
                    Err(message) => {
                        eprintln!(
                            "{}",
                            serde_json::json!({
                                "component": "remote_session_sync",
                                "kind": "coordinator.spawn_failed",
                                "target": target.id,
                                "message": message,
                            })
                        );
                        let changed = lock_recover(&runtime).ingest_remote_session(
                            &target.id,
                            Err(crate::live::SessionFetchError::Unreachable(message)),
                        );
                        if changed {
                            notifier.notify();
                        }
                    }
                }
            }
        }
        Box::into_raw(Box::new(HerdrCore {
            _session_sync: session_sync,
            _remote_session_sync: remote_session_sync,
            runtime,
            notifier,
            owner_thread: thread::current().id(),
        }))
    }))
    .unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_dispatch(core: *mut HerdrCore, event_json: *const u8, len: usize) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(core) = core_ref(core) else {
            return;
        };
        if !check_owner_thread(core, "dispatch") {
            notify_change(core);
            return;
        }
        let Some(bytes) = input_bytes(event_json, len) else {
            lock_recover(&core.runtime).set_error(
                "event.invalid_pointer",
                "Event pointer was null for a non-empty payload",
                false,
            );
            notify_change(core);
            return;
        };
        let changed = lock_recover(&core.runtime).dispatch_json(bytes);
        if changed {
            notify_change(core);
        }
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
        if !check_owner_thread(core, "snapshot") {
            notify_change(core);
            return HerdrBytes::empty();
        }
        // Clear first, then read. A change landing between the two announces
        // itself again and costs one extra read; clearing after the read would
        // lose it.
        core.notifier.clear_announcement();
        let payload = {
            let mut runtime = lock_recover(&core.runtime);
            runtime.snapshot_delta_payload(have_revision, have_terminal_sequence)
        };
        // The guard is gone before a byte is written. Serializing the
        // navigator, ui state and terminal output under the lock made every
        // attach thread and the next read wait behind it.
        match crate::runtime::serialize_snapshot_delta(&payload) {
            Ok(bytes) => HerdrBytes::from_vec(bytes),
            Err(_) => HerdrBytes::empty(),
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
        core.notifier
            .set_callback(callback.map(|callback| CallbackRegistration { callback, context }));
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
        panic!("every pipe was inherited by a concurrently spawned fixture, so the write under test never happened");
    }
}
