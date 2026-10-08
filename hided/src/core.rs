//! Owner-thread wrapper around `herdr_core::Core`.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use herdr_core::node_access::NodeLink;
use herdr_core::workspace_control::{
    Action, ActionMaterial, ActionPreparation, ActionResult, Context, Query, QueryResult, Refusal,
};
use herdr_core::{Core, CoreOptions};
use hide_node::terminal::device::DeviceSink;
use hide_node::terminal::router::Router;
use hide_node::terminal::{
    Attacher, LocalAttacher, Mode, OutputSink, ReportSink, RetryPolicy, Service, SessionParts,
};
use tokio::sync::broadcast;

use crate::terminal_hub::TerminalHub;

pub struct SnapshotReply {
    pub bytes: Vec<u8>,
}

enum Command {
    FactoryQuestionGuard {
        device_id: String,
        pane_id: String,
        expected: Context,
        session: String,
        agent_runtime: String,
        terminal_id: String,
        deadline: std::time::Instant,
        reply: Sender<Result<herdr_core::factory::PreparedQuestionGuard, String>>,
    },
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
        reply: Sender<Result<SnapshotReply, String>>,
    },
    Shutdown,
}

pub struct CoreHandle {
    /// The machine the core runs on, named in every key for this machine.
    node: herdr_core::node::NodeId,
    commands: Sender<Command>,
    pub notify: broadcast::Sender<()>,
    /// Every node's terminals: a screen's keys and views go here directly,
    /// never through the core (PRD core-host-node-terminal D-05).
    pub terminals: Arc<Router>,
    /// The output every node's terminals produce, as each screen reads it.
    pub hub: Arc<TerminalHub>,
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

    #[allow(clippy::too_many_arguments)] // carry the authenticated pane facts and caller-owned deadline unchanged
    pub fn prepare_factory_question_guard(
        &self,
        device: &str,
        pane: &str,
        expected: &Context,
        session: String,
        agent_runtime: String,
        terminal_id: String,
        deadline: std::time::Instant,
    ) -> Result<herdr_core::factory::PreparedQuestionGuard, String> {
        let (reply, result) = mpsc::channel();
        let left = deadline
            .checked_duration_since(std::time::Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or("factory_guard_expired")?;
        self.commands
            .send(Command::FactoryQuestionGuard {
                device_id: device.to_owned(),
                pane_id: pane.to_owned(),
                expected: expected.clone(),
                session,
                agent_runtime,
                terminal_id,
                deadline,
                reply,
            })
            .map_err(|_| "factory_guard_unavailable")?;
        result
            .recv_timeout(left)
            .map_err(|_| "factory_guard_expired")?
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

    /// `panes` is where each device's node link sends its panes' proofs and
    /// command streams (`node_panes`).
    pub fn spawn(
        options: CoreOptions,
        panes: hide_node::ssh::PaneEventsSlot,
    ) -> Result<Self, String> {
        let node = options.node_id.clone();
        let (command_tx, command_rx) = mpsc::channel::<Command>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let (notify_tx, _) = broadcast::channel(32);
        let notify_for_thread = notify_tx.clone();
        let (reports, terminal_reports) = herdr_core::terminal_reports::terminal_reports();
        let reports: Arc<dyn ReportSink> = Arc::new(reports);
        let hub = TerminalHub::new();
        let local = Service::start(
            local_attacher(&options),
            Arc::clone(&hub) as Arc<dyn OutputSink>,
            Arc::clone(&reports),
            RetryPolicy::Automatic,
        )
        .map_err(|error| format!("terminal service failed to start: {error}"))?;
        let terminals = Arc::new(Router::new(
            Arc::new(local),
            Arc::clone(&hub) as Arc<dyn OutputSink>,
            reports,
        ));
        let routes = Arc::clone(&terminals);
        let thread = thread::Builder::new()
            .name("hided-core".into())
            .spawn(move || {
                owner_loop(
                    options,
                    panes,
                    (routes, terminal_reports),
                    command_rx,
                    ready_tx,
                    notify_for_thread,
                )
            })
            .map_err(|error| format!("core owner thread failed to start: {error}"))?;
        ready_rx
            .recv()
            .map_err(|_| "core owner thread exited before ready".to_owned())??;
        Ok(Self {
            node,
            commands: command_tx,
            notify: notify_tx,
            terminals,
            hub,
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

    /// The link to the node `device_id` names; see `Core::node_link`.
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

    pub fn workspace_query_until(
        &self,
        device_id: &str,
        pane_id: &str,
        query: Query,
        deadline: std::time::Instant,
    ) -> Result<QueryResult, Refusal> {
        let unavailable = || Refusal {
            reason: "factory_guard_expired",
            next_action: "Continue the tool call",
        };
        let left = deadline
            .checked_duration_since(std::time::Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(unavailable)?;
        let (reply, rx) = mpsc::channel();
        self.commands
            .send(Command::WorkspaceQuery {
                device_id: device_id.to_owned(),
                pane_id: pane_id.to_owned(),
                query,
                reply,
            })
            .map_err(|_| unavailable())?;
        rx.recv_timeout(left).map_err(|_| unavailable())?
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

    pub fn snapshot(&self, have_revision: u64) -> Result<SnapshotReply, String> {
        let (reply, rx) = mpsc::channel();
        self.commands
            .send(Command::Snapshot {
                have_revision,
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

/// This machine's Herdr, through the official `herdr terminal session`
/// client. A daemon with no Herdr server has no panes of its own, and an
/// attach says so.
fn local_attacher(options: &CoreOptions) -> Box<dyn Attacher> {
    struct NoHerdr;
    impl Attacher for NoHerdr {
        fn open(&self, _: &str, _: Mode, _: u16, _: u16) -> Result<SessionParts, String> {
            Err("This daemon runs without a Herdr server".to_owned())
        }
    }
    match options.herdr_socket_path.as_deref() {
        Some(socket) => Box::new(LocalAttacher::new(
            options
                .herdr_bin_path
                .as_deref()
                .map(std::path::PathBuf::from),
            std::path::PathBuf::from(socket),
        )),
        None => Box::new(NoHerdr),
    }
}

fn owner_loop(
    options: CoreOptions,
    panes: hide_node::ssh::PaneEventsSlot,
    (terminals, terminal_reports): (Arc<Router>, herdr_core::terminal_reports::TerminalReports),
    commands: Receiver<Command>,
    ready: Sender<Result<(), String>>,
    notify: broadcast::Sender<()>,
) {
    // The core's own node answers for the home the core reads and writes
    // for: the configured one, else the process's, as the core decides.
    let mut own_node = hide_node::Local::new(
        options
            .home
            .as_ref()
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from)),
    );
    // The install kit's parts ship beside this binary in the app bundle; a
    // daemon anywhere else installs nothing (PRD device-parity D-19).
    if let Some(kit_dir) = std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(hide_kit::bundled_kit_dir)
    {
        own_node = own_node.bundled(kit_dir);
    }
    let own_herdr = options
        .herdr_socket_path
        .as_deref()
        .map(|socket| hide_node::herdr(std::path::Path::new(socket)));
    // The program builds devices run ship beside this binary; a daemon
    // anywhere else carries none, which leaves each device's node
    // `unsupported` with that reason.
    let devices = hide_node::ssh::Connector::new(
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf)),
    )
    .with_pane_events(panes)
    .with_terminals(Arc::clone(&terminals) as Arc<dyn DeviceSink>);
    let Some(core) = Core::create(
        options,
        std::sync::Arc::new(own_node),
        own_herdr,
        std::sync::Arc::new(devices),
        terminals,
        terminal_reports,
    ) else {
        let _ = ready.send(Err(
            "herdr-core create failed (check schema_version and paths)".to_owned(),
        ));
        return;
    };
    // After the core opened its Logs file: the SSH transport's records land
    // there too.
    hide_node::diagnostics::install(herdr_core::diagnostics::emit);
    core.on_change(move || {
        let _ = notify.send(());
    });
    let _ = ready.send(Ok(()));
    let mut _held_roots = hide_node::HeldRoots::default();
    while let Ok(command) = commands.recv() {
        match command {
            Command::FactoryQuestionGuard {
                device_id,
                pane_id,
                expected,
                session,
                agent_runtime,
                terminal_id,
                deadline,
                reply,
            } => {
                let _ = reply.send(core.prepare_factory_question_guard(
                    &device_id,
                    &pane_id,
                    &expected,
                    &session,
                    &agent_runtime,
                    &terminal_id,
                    deadline,
                ));
            }
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
                // This node holds the opened roots while the core pins their
                // identities; the previous set closes once it is replaced.
                let (held, identities) = hide_node::hold_roots(roots);
                core.set_file_roots(herdr_core::FileRoots::from_identities(identities));
                _held_roots = held;
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
                reply,
            } => {
                let bytes = core.snapshot_delta(have_revision);
                let _ = reply.send(Ok(SnapshotReply { bytes }));
            }
            Command::Shutdown => break,
        }
    }
    core.clear_on_change();
}
