//! Owner-thread wrapper around `herdr_core::Core`.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use herdr_core::host_access::HostChannel;
use herdr_core::workspace_control::{Query, QueryResult, Refusal};
use herdr_core::{Core, CoreOptions};
use tokio::sync::broadcast;

pub struct SnapshotReply {
    pub bytes: Vec<u8>,
}

enum Command {
    SetFileRoots {
        roots: Vec<(std::path::PathBuf, std::fs::File)>,
        reply: Sender<Result<(), String>>,
    },
    Dispatch {
        event: Vec<u8>,
        reply: Sender<Result<(), String>>,
    },
    DeviceChannel {
        device_id: String,
        reply: Sender<Result<Arc<dyn HostChannel>, String>>,
    },
    WorkspaceQuery {
        device_id: String,
        pane_id: String,
        query: Query,
        reply: Sender<Result<QueryResult, Refusal>>,
    },
    Snapshot {
        have_revision: u64,
        have_terminal_sequence: u64,
        reply: Sender<Result<SnapshotReply, String>>,
    },
    Shutdown,
}

pub struct CoreHandle {
    commands: Sender<Command>,
    pub notify: broadcast::Sender<()>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl CoreHandle {
    pub fn set_file_roots(
        &self,
        roots: Vec<(std::path::PathBuf, std::fs::File)>,
    ) -> Result<(), String> {
        let (reply, rx) = mpsc::channel();
        self.commands
            .send(Command::SetFileRoots { roots, reply })
            .map_err(|_| "core owner thread is gone".to_owned())?;
        rx.recv()
            .map_err(|_| "core owner thread dropped file-root reply".to_owned())?
    }

    pub fn spawn(options: CoreOptions) -> Result<Self, String> {
        let (command_tx, command_rx) = mpsc::channel::<Command>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let (notify_tx, _) = broadcast::channel(32);
        let notify_for_thread = notify_tx.clone();
        let thread = thread::Builder::new()
            .name("hided-core".into())
            .spawn(move || owner_loop(options, command_rx, ready_tx, notify_for_thread))
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

    /// Where a device's file work runs; see `Core::device_channel`.
    pub fn device_channel(&self, device_id: &str) -> Result<Arc<dyn HostChannel>, String> {
        let (reply, rx) = mpsc::channel();
        self.commands
            .send(Command::DeviceChannel {
                device_id: device_id.to_owned(),
                reply,
            })
            .map_err(|_| "core owner thread is gone".to_owned())?;
        rx.recv()
            .map_err(|_| "core owner thread dropped device-channel reply".to_owned())?
    }

    pub fn workspace_query(
        &self,
        device_id: &str,
        pane_id: &str,
        query: Query,
    ) -> Result<QueryResult, Refusal> {
        let (reply, rx) = mpsc::channel();
        self.commands
            .send(Command::WorkspaceQuery {
                device_id: device_id.to_owned(),
                pane_id: pane_id.to_owned(),
                query,
                reply,
            })
            .map_err(|_| Refusal {
                reason: "core_unavailable",
                next_action: "Reconnect Hide and retry",
            })?;
        rx.recv().map_err(|_| Refusal {
            reason: "core_unavailable",
            next_action: "Reconnect Hide and retry",
        })?
    }

    pub fn snapshot(
        &self,
        have_revision: u64,
        have_terminal_sequence: u64,
    ) -> Result<SnapshotReply, String> {
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
        if let Ok(mut thread) = self.thread.lock()
            && let Some(thread) = thread.take()
        {
            let _ = thread.join();
        }
    }
}

impl Drop for CoreHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn owner_loop(
    options: CoreOptions,
    commands: Receiver<Command>,
    ready: Sender<Result<(), String>>,
    notify: broadcast::Sender<()>,
) {
    let Some(core) = Core::create(options) else {
        let _ = ready.send(Err(
            "herdr-core create failed (check schema_version and paths)".to_owned(),
        ));
        return;
    };
    core.on_change(move || {
        let _ = notify.send(());
    });
    let _ = ready.send(Ok(()));
    while let Ok(command) = commands.recv() {
        match command {
            Command::SetFileRoots { roots, reply } => {
                core.set_file_roots(herdr_core::FileRoots::from_opened(roots));
                let _ = reply.send(Ok(()));
            }
            Command::Dispatch { event, reply } => {
                let _ = core.dispatch_bytes(&event);
                let _ = reply.send(Ok(()));
            }
            Command::DeviceChannel { device_id, reply } => {
                let _ = reply.send(core.device_channel(&device_id));
            }
            Command::WorkspaceQuery {
                device_id,
                pane_id,
                query,
                reply,
            } => {
                let _ = reply.send(core.workspace_control_query(&device_id, &pane_id, query));
            }
            Command::Snapshot {
                have_revision,
                have_terminal_sequence,
                reply,
            } => {
                let bytes = core.snapshot_delta(have_revision, have_terminal_sequence);
                let _ = reply.send(Ok(SnapshotReply { bytes }));
            }
            Command::Shutdown => break,
        }
    }
    core.clear_on_change();
}
