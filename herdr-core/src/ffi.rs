use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::slice;
use std::sync::{Mutex, MutexGuard};
use std::thread::{self, ThreadId};

use crate::model::CoreOptions;
use crate::runtime::{Runtime, validate_options};

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

#[repr(C)]
pub struct HerdrCore {
    runtime: Mutex<Runtime>,
    callback: Mutex<Option<CallbackRegistration>>,
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
    let registration = *lock_recover(&core.callback);
    if let Some(registration) = registration {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            (registration.callback)(registration.context);
        }));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn herdr_core_create(options_json: *const u8, len: usize) -> *mut HerdrCore {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(bytes) = input_bytes(options_json, len) else {
            return ptr::null_mut();
        };
        let Ok(options) = serde_json::from_slice::<CoreOptions>(bytes) else {
            return ptr::null_mut();
        };
        if validate_options(&options).is_err() {
            return ptr::null_mut();
        }
        Box::into_raw(Box::new(HerdrCore {
            runtime: Mutex::new(Runtime::new(options)),
            callback: Mutex::new(None),
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
pub extern "C" fn herdr_core_snapshot(core: *mut HerdrCore) -> HerdrBytes {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(core) = core_ref(core) else {
            return HerdrBytes::empty();
        };
        let _ = check_owner_thread(core, "snapshot");
        match serde_json::to_vec(lock_recover(&core.runtime).snapshot()) {
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
        }
        *lock_recover(&core_ref.callback) = None;
        unsafe {
            drop_core(core);
        }
    }));
}
