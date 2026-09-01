use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::slice;
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
#[derive(Clone)]
pub struct ChangeNotifier {
    registration: Arc<Mutex<Option<CallbackRegistration>>>,
}

impl ChangeNotifier {
    pub fn notify(&self) {
        let registration = *lock_recover(&self.registration);
        if let Some(registration) = registration {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                (registration.callback)(registration.context);
            }));
        }
    }

    #[cfg(test)]
    pub(crate) fn noop() -> Self {
        Self {
            registration: Arc::new(Mutex::new(None)),
        }
    }
}

#[repr(C)]
pub struct HerdrCore {
    _session_sync: Option<crate::session_sync::SessionSyncHandle>,
    _remote_session_sync: Vec<crate::session_sync::SessionSyncHandle>,
    runtime: Arc<Mutex<Runtime>>,
    callback: Arc<Mutex<Option<CallbackRegistration>>>,
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
    ChangeNotifier {
        registration: Arc::clone(&core.callback),
    }
    .notify();
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_create(options_json: *const u8, len: usize) -> *mut HerdrCore {
    catch_unwind(AssertUnwindSafe(|| {
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
        let callback = Arc::new(Mutex::new(None));
        lock_recover(&runtime).install_worker_context(
            Arc::downgrade(&runtime),
            ChangeNotifier {
                registration: Arc::clone(&callback),
            },
        );
        let session_sync = if let Some(socket_path) = options.herdr_socket_path.as_deref() {
            live::install(
                &runtime,
                ChangeNotifier {
                    registration: Arc::clone(&callback),
                },
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
                            ChangeNotifier {
                                registration: Arc::clone(&callback),
                            },
                        ),
                    );
                    lock_recover(&runtime).install_remote_terminal(
                        crate::live::RemoteTerminalContext::new(
                            target.id.clone(),
                            Arc::clone(&client),
                            target.herdr_socket_path.clone(),
                            Arc::downgrade(&runtime),
                            ChangeNotifier {
                                registration: Arc::clone(&callback),
                            },
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
                        ChangeNotifier {
                            registration: Arc::clone(&callback),
                        },
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
                            ChangeNotifier {
                                registration: Arc::clone(&callback),
                            }
                            .notify();
                        }
                    }
                }
            }
        }
        Box::into_raw(Box::new(HerdrCore {
            _session_sync: session_sync,
            _remote_session_sync: remote_session_sync,
            runtime,
            callback,
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
        match lock_recover(&core.runtime).snapshot_delta(have_revision, have_terminal_sequence) {
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
        *lock_recover(&core.callback) =
            callback.map(|callback| CallbackRegistration { callback, context });
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
        *lock_recover(&core_ref.callback) = None;
        unsafe {
            drop_core(core);
        }
    }));
}
