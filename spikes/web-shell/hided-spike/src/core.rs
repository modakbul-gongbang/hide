//! Owner-thread wrapper around the herdr-core C ABI.
//!
//! create, dispatch, snapshot, on_change, and destroy must run on the thread
//! that created the core. Axum workers therefore send commands here.

use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use herdr_core::{
    herdr_core_create, herdr_core_destroy, herdr_core_dispatch, herdr_core_free_bytes,
    herdr_core_on_change, herdr_core_snapshot,
};

pub enum Command {
    Dispatch {
        event: Vec<u8>,
        reply: Sender<Result<(), String>>,
    },
    Snapshot {
        have_revision: u64,
        have_terminal_sequence: u64,
        reply: Sender<Result<Vec<u8>, String>>,
    },
    Shutdown,
}

pub struct CoreHandle {
    commands: Sender<Command>,
    pub notify: tokio::sync::broadcast::Sender<()>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

struct NotifyContext {
    tx: tokio::sync::broadcast::Sender<()>,
}

extern "C" fn on_change(context: *mut c_void) {
    if context.is_null() {
        return;
    }
    let ctx = unsafe { &*(context as *const NotifyContext) };
    let _ = ctx.tx.send(());
}

impl CoreHandle {
    pub fn spawn(options_json: Vec<u8>) -> Result<Self, String> {
        let (command_tx, command_rx) = mpsc::channel::<Command>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let (notify_tx, _) = tokio::sync::broadcast::channel(32);
        let notify_for_thread = notify_tx.clone();
        let thread = thread::Builder::new()
            .name("hided-spike-core".into())
            .spawn(move || {
                owner_loop(options_json, command_rx, ready_tx, notify_for_thread);
            })
            .map_err(|error| format!("core owner thread failed to start: {error}"))?;
        ready_rx
            .recv()
            .map_err(|_| "core owner thread exited before ready".to_owned())??;
        Ok(Self {
            commands: command_tx,
            notify: notify_tx,
            thread: Mutex::new(Some(thread)),
        })
    }

    pub fn dispatch(&self, event: Vec<u8>) -> Result<(), String> {
        let (reply, rx) = mpsc::channel();
        self.commands
            .send(Command::Dispatch { event, reply })
            .map_err(|_| "core owner thread is gone".to_owned())?;
        rx.recv()
            .map_err(|_| "core owner thread dropped dispatch reply".to_owned())?
    }

    pub fn snapshot(
        &self,
        have_revision: u64,
        have_terminal_sequence: u64,
    ) -> Result<Vec<u8>, String> {
        let (reply, rx) = mpsc::channel();
        self.commands
            .send(Command::Snapshot {
                have_revision,
                have_terminal_sequence,
                reply,
            })
            .map_err(|_| "core owner thread is gone".to_owned())?;
        rx.recv()
            .map_err(|_| "core owner thread dropped snapshot reply".to_owned())?
    }

    pub fn shutdown(&self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Ok(mut thread) = self.thread.lock() {
            if let Some(thread) = thread.take() {
                let _ = thread.join();
            }
        }
    }
}

impl Drop for CoreHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn owner_loop(
    options_json: Vec<u8>,
    commands: Receiver<Command>,
    ready: Sender<Result<(), String>>,
    notify: tokio::sync::broadcast::Sender<()>,
) {
    let core = herdr_core_create(options_json.as_ptr(), options_json.len());
    if core.is_null() {
        let _ = ready.send(Err(
            "herdr_core_create returned null (check schema_version and paths)".to_owned(),
        ));
        return;
    }
    let notify_ctx = Box::new(NotifyContext { tx: notify });
    let ctx_ptr = Box::into_raw(notify_ctx);
    herdr_core_on_change(core, Some(on_change), ctx_ptr as *mut c_void);
    let _ = ready.send(Ok(()));
    while let Ok(command) = commands.recv() {
        match command {
            Command::Dispatch { event, reply } => {
                herdr_core_dispatch(core, event.as_ptr(), event.len());
                let _ = reply.send(Ok(()));
            }
            Command::Snapshot {
                have_revision,
                have_terminal_sequence,
                reply,
            } => {
                let bytes = herdr_core_snapshot(core, have_revision, have_terminal_sequence);
                let copy = if bytes.ptr.is_null() {
                    Vec::new()
                } else {
                    unsafe { std::slice::from_raw_parts(bytes.ptr, bytes.len) }.to_vec()
                };
                herdr_core_free_bytes(bytes);
                let _ = reply.send(Ok(copy));
            }
            Command::Shutdown => break,
        }
    }
    herdr_core_on_change(core, None, std::ptr::null_mut());
    herdr_core_destroy(core);
    unsafe {
        drop(Box::from_raw(ctx_ptr));
    }
}
