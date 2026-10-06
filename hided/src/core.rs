//! Owner-thread wrapper around `herdr_core::Core`.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use herdr_core::node_access::NodeLink;
use herdr_core::workspace_control::{
    Action, ActionMaterial, ActionPreparation, ActionResult, Context, Query, QueryResult, Refusal,
};
use herdr_core::{Core, CoreOptions};
use tokio::sync::broadcast;

pub struct SnapshotReply {
    pub bytes: Vec<u8>,
}

enum Command {
    DeliveryHuman {
        reply: Sender<Result<herdr_core::delivery::worker::PreparedHuman, String>>,
    },
    DeliveryPrepare {
        device_id: String,
        pane_id: String,
        expected: Context,
        hint: Option<String>,
        command: herdr_core::delivery::Command,
        reply: Sender<Result<herdr_core::delivery::worker::Prepared, String>>,
    },
    FactoryPrepare {
        device_id: String,
        pane_id: String,
        expected: Context,
        hint: Option<String>,
        command: hide_factory::Command,
        reply: Sender<Result<herdr_core::factory::PreparedFactory, String>>,
    },
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
        reply: Sender<Result<Arc<dyn NodeLink>, String>>,
    },
    WorkspaceRemoteRoutes {
        reply: Sender<Vec<herdr_core::WorkspaceRemoteRoute>>,
    },
    RemoteHerdrApi {
        device_id: String,
        reply: Sender<Option<Arc<dyn hide_herdr_client::ApiConnector>>>,
    },
    BrowserRouteSource {
        device_id: String,
        checkout_path: String,
        view_id: String,
        load: u64,
        reply: Sender<Option<herdr_core::workspace_control::BrowserRouteSource>>,
    },
    WorkspaceQuery {
        device_id: String,
        pane_id: String,
        query: Query,
        reply: Sender<Result<QueryResult, Refusal>>,
    },
    LinksScope {
        context: Context,
        reply: Sender<Option<herdr_core::links::query::Scope>>,
    },
    WorkspaceAction {
        device_id: String,
        pane_id: String,
        expected: Context,
        request_id: String,
        action: Action,
        material: Box<Result<Option<ActionMaterial>, Refusal>>,
        reply: Sender<Result<ActionResult, Refusal>>,
    },
    WorkspacePrepare {
        device_id: String,
        pane_id: String,
        expected: Context,
        request_id: String,
        action: Action,
        reply: Sender<Result<ActionPreparation, Refusal>>,
    },
    Snapshot {
        have_revision: u64,
        have_terminal_sequence: u64,
        reply: Sender<Result<SnapshotReply, String>>,
    },
    Shutdown,
}

pub struct CoreHandle {
    /// The machine the core runs on, named in every key for this machine.
    node: herdr_core::node::NodeId,
    commands: Sender<Command>,
    pub notify: broadcast::Sender<()>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl CoreHandle {
    /// The machine the core runs on.
    pub fn node(&self) -> &herdr_core::node::NodeId {
        &self.node
    }

    pub fn prepare_delivery_human(
        &self,
    ) -> Result<herdr_core::delivery::worker::PreparedHuman, String> {
        let (reply, result) = mpsc::channel();
        self.commands
            .send(Command::DeliveryHuman { reply })
            .map_err(|_| "delivery_unavailable")?;
        result.recv().map_err(|_| "delivery_unavailable")?
    }
    pub fn prepare_delivery(
        &self,
        device: &str,
        pane: &str,
        expected: &Context,
        hint: Option<String>,
        command: herdr_core::delivery::Command,
    ) -> Result<herdr_core::delivery::worker::Prepared, String> {
        let (reply, result) = mpsc::channel();
        self.commands
            .send(Command::DeliveryPrepare {
                device_id: device.to_owned(),
                pane_id: pane.to_owned(),
                expected: expected.clone(),
                hint,
                command,
                reply,
            })
            .map_err(|_| "delivery_unavailable".to_owned())?;
        result
            .recv()
            .map_err(|_| "delivery_unavailable".to_owned())?
    }

    /// A `hide factory` command, checked on the owner thread like a delivery.
    pub fn prepare_factory(
        &self,
        device: &str,
        pane: &str,
        expected: &Context,
        hint: Option<String>,
        command: hide_factory::Command,
    ) -> Result<herdr_core::factory::PreparedFactory, String> {
        let (reply, result) = mpsc::channel();
        self.commands
            .send(Command::FactoryPrepare {
                device_id: device.to_owned(),
                pane_id: pane.to_owned(),
                expected: expected.clone(),
                hint,
                command,
                reply,
            })
            .map_err(|_| "factory_unavailable".to_owned())?;
        result
            .recv()
            .map_err(|_| "factory_unavailable".to_owned())?
    }

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
        let node = options.node_id.clone();
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
            node,
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

    /// Where a device's file work runs; see `Core::node_link`.
    pub fn node_link(&self, device_id: &str) -> Result<Arc<dyn NodeLink>, String> {
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

    pub fn workspace_remote_routes(&self) -> Result<Vec<herdr_core::WorkspaceRemoteRoute>, String> {
        let (reply, rx) = mpsc::channel();
        self.commands
            .send(Command::WorkspaceRemoteRoutes { reply })
            .map_err(|_| "core owner thread is gone".to_owned())?;
        rx.recv()
            .map_err(|_| "core owner thread dropped remote routes".to_owned())
    }

    /// The Herdr API connection the core holds for a connected SSH device;
    /// see `Core::remote_herdr_api`.
    pub fn remote_herdr_api(
        &self,
        device_id: &str,
    ) -> Result<Option<Arc<dyn hide_herdr_client::ApiConnector>>, String> {
        let (reply, rx) = mpsc::channel();
        self.commands
            .send(Command::RemoteHerdrApi {
                device_id: device_id.to_owned(),
                reply,
            })
            .map_err(|_| "core owner thread is gone".to_owned())?;
        rx.recv()
            .map_err(|_| "core owner thread dropped the device connection reply".to_owned())
    }

    pub fn browser_route_source(
        &self,
        device_id: &str,
        checkout_path: &str,
        view_id: &str,
        load: u64,
    ) -> Result<Option<herdr_core::workspace_control::BrowserRouteSource>, String> {
        let (reply, rx) = mpsc::channel();
        self.commands
            .send(Command::BrowserRouteSource {
                device_id: device_id.to_owned(),
                checkout_path: checkout_path.to_owned(),
                view_id: view_id.to_owned(),
                load,
                reply,
            })
            .map_err(|_| "core owner thread is gone".to_owned())?;
        rx.recv()
            .map_err(|_| "core owner thread dropped browser route source".to_owned())
    }

    /// The Projects a `hide links` read may answer from, as the core knows
    /// them now; the read itself runs on the caller's blocking task.
    pub fn links_scope(&self, context: &Context) -> Option<herdr_core::links::query::Scope> {
        let (reply, rx) = mpsc::channel();
        self.commands
            .send(Command::LinksScope {
                context: context.clone(),
                reply,
            })
            .ok()?;
        rx.recv().ok().flatten()
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

    pub fn workspace_action(
        &self,
        device_id: &str,
        pane_id: &str,
        expected: &Context,
        request_id: &str,
        action: Action,
        material: Result<Option<ActionMaterial>, Refusal>,
    ) -> Result<ActionResult, Refusal> {
        let (reply, rx) = mpsc::channel();
        self.commands
            .send(Command::WorkspaceAction {
                device_id: device_id.to_owned(),
                pane_id: pane_id.to_owned(),
                expected: expected.clone(),
                request_id: request_id.to_owned(),
                action,
                material: Box::new(material),
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

    pub fn workspace_prepare_action(
        &self,
        device_id: &str,
        pane_id: &str,
        expected: &Context,
        request_id: &str,
        action: &Action,
    ) -> Result<ActionPreparation, Refusal> {
        let (reply, rx) = mpsc::channel();
        self.commands
            .send(Command::WorkspacePrepare {
                device_id: device_id.to_owned(),
                pane_id: pane_id.to_owned(),
                expected: expected.clone(),
                request_id: request_id.to_owned(),
                action: action.clone(),
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
    let Some(core) = Core::create(options, std::sync::Arc::new(hide_node::Local)) else {
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
            Command::DeliveryHuman { reply } => {
                let _ = reply.send(core.prepare_delivery_human());
            }
            Command::FactoryPrepare {
                device_id,
                pane_id,
                expected,
                hint,
                command,
                reply,
            } => {
                let _ = reply.send(core.prepare_factory(
                    &device_id,
                    &pane_id,
                    &expected,
                    hint.as_deref(),
                    command,
                ));
            }
            Command::DeliveryPrepare {
                device_id,
                pane_id,
                expected,
                hint,
                command,
                reply,
            } => {
                let _ = reply.send(core.prepare_delivery(
                    &device_id,
                    &pane_id,
                    &expected,
                    hint.as_deref(),
                    command,
                ));
            }
            Command::SetFileRoots { roots, reply } => {
                core.set_file_roots(herdr_core::FileRoots::from_opened(roots));
                let _ = reply.send(Ok(()));
            }
            Command::Dispatch { event, reply } => {
                let _ = core.dispatch_bytes(&event);
                let _ = reply.send(Ok(()));
            }
            Command::DeviceChannel { device_id, reply } => {
                let _ = reply.send(core.node_link(&device_id));
            }
            Command::WorkspaceRemoteRoutes { reply } => {
                let _ = reply.send(core.workspace_remote_routes());
            }
            Command::RemoteHerdrApi { device_id, reply } => {
                let _ = reply.send(core.remote_herdr_api(&device_id));
            }
            Command::BrowserRouteSource {
                device_id,
                checkout_path,
                view_id,
                load,
                reply,
            } => {
                let _ = reply.send(core.browser_route_source(
                    &device_id,
                    &checkout_path,
                    &view_id,
                    load,
                ));
            }
            Command::WorkspaceQuery {
                device_id,
                pane_id,
                query,
                reply,
            } => {
                let _ = reply.send(core.workspace_control_query(&device_id, &pane_id, query));
            }
            Command::LinksScope { context, reply } => {
                let _ = reply.send(core.links_scope(&context));
            }
            Command::WorkspaceAction {
                device_id,
                pane_id,
                expected,
                request_id,
                action,
                material,
                reply,
            } => {
                let _ = reply.send(core.workspace_control_action(
                    &device_id,
                    &pane_id,
                    &expected,
                    &request_id,
                    action,
                    *material,
                ));
            }
            Command::WorkspacePrepare {
                device_id,
                pane_id,
                expected,
                request_id,
                action,
                reply,
            } => {
                let _ = reply.send(core.workspace_control_prepare_action(
                    &device_id,
                    &pane_id,
                    &expected,
                    &request_id,
                    &action,
                ));
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
