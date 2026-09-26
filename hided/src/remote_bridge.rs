//! SSH device return routes for pane-scoped Workspace commands. The core
//! selects consented device/helper pairs; this supervisor owns one reverse
//! loopback forward and one attesting helper exec channel for each pair.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use herdr_core::WorkspaceRemoteRoute;
use hide_host::workspace_bridge::PaneIdentity;
use serde_json::{Value, json};

use crate::core::CoreHandle;
use crate::pane_auth::{self, Registry};
use crate::state_file::new_token;

const MAX_ROUTES: usize = 8;

type Shutdown = Box<dyn FnOnce() + Send>;

struct RouteContext<'a> {
    core: &'a CoreHandle,
    registry: &'a Registry,
    port: u16,
    bridge_dir: Option<PathBuf>,
}

struct WorkerStop {
    cancelled: AtomicBool,
    shutdown: Mutex<Option<Shutdown>>,
}

impl WorkerStop {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            shutdown: Mutex::new(None),
        }
    }

    fn install(&self, shutdown: Shutdown) {
        let Ok(mut slot) = self.shutdown.lock() else {
            return;
        };
        *slot = Some(shutdown);
        if self.cancelled.load(Ordering::Acquire)
            && let Some(shutdown) = slot.take()
        {
            shutdown();
        }
    }

    fn stop(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Ok(mut slot) = self.shutdown.lock()
            && let Some(shutdown) = slot.take()
        {
            shutdown();
        }
    }
}

struct Worker {
    generation: u64,
    stop: Arc<WorkerStop>,
    thread: JoinHandle<()>,
}

pub struct Supervisor {
    closed: AtomicBool,
    workers: Mutex<HashMap<String, Worker>>,
}

impl Supervisor {
    pub fn spawn(
        core: Arc<CoreHandle>,
        registry: Arc<Registry>,
        port: u16,
        bridge_dir: Option<PathBuf>,
    ) -> Arc<Self> {
        let supervisor = Arc::new(Self {
            closed: AtomicBool::new(false),
            workers: Mutex::new(HashMap::new()),
        });
        let running = Arc::clone(&supervisor);
        tokio::spawn(async move {
            let mut poll = tokio::time::interval(Duration::from_secs(2));
            loop {
                poll.tick().await;
                if running.closed.load(Ordering::Acquire) {
                    break;
                }
                let query_core = Arc::clone(&core);
                let routes =
                    tokio::task::spawn_blocking(move || query_core.workspace_remote_routes()).await;
                let Ok(Ok(routes)) = routes else { continue };
                running.reconcile(routes, &core, &registry, port, &bridge_dir);
            }
        });
        supervisor
    }

    fn reconcile(
        &self,
        routes: Vec<WorkspaceRemoteRoute>,
        core: &Arc<CoreHandle>,
        registry: &Arc<Registry>,
        port: u16,
        bridge_dir: &Option<PathBuf>,
    ) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let Ok(mut workers) = self.workers.lock() else {
            return;
        };
        let current: HashMap<_, _> = routes
            .iter()
            .map(|route| (route.device_id.as_str(), route.generation))
            .collect();
        workers.retain(|device, worker| {
            let keep = current.get(device.as_str()) == Some(&worker.generation)
                && !worker.thread.is_finished();
            if !keep {
                worker.stop.stop();
            }
            keep
        });
        for route in routes.into_iter().take(MAX_ROUTES) {
            if workers.contains_key(&route.device_id) {
                continue;
            }
            let stop = Arc::new(WorkerStop::new());
            let device = route.device_id.clone();
            let generation = route.generation;
            let core = Arc::clone(core);
            let registry = Arc::clone(registry);
            let worker_stop = Arc::clone(&stop);
            let bridge_dir = bridge_dir.clone();
            match std::thread::Builder::new().name("hided-workspace-bridge".to_owned()).spawn(move || {
                let bridge_id = format!("{}:{}:{}", route.device_id, route.generation, new_token());
                let alive = Arc::new(AtomicBool::new(true));
                let context = RouteContext { core: &core, registry: &registry, port, bridge_dir };
                if let Err(reason) = run_route(&route, &bridge_id, Arc::clone(&alive), &worker_stop, &context) {
                    eprintln!("{}", json!({"component":"workspace_bridge","kind":"route.failed","device_id":route.device_id,"reason":reason}));
                }
                alive.store(false, Ordering::Release);
                registry.revoke_bridge(&bridge_id);
                worker_stop.stop();
            }) {
                Ok(thread) => { workers.insert(device, Worker { generation, stop, thread }); }
                Err(error) => {
                    eprintln!("{}", json!({"component":"workspace_bridge","kind":"worker.failed","device_id":device,"reason":error.to_string()}));
                }
            }
        }
    }

    pub fn stop_all(&self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(mut workers) = self.workers.lock() {
            for worker in workers.values() {
                worker.stop.stop();
            }
            workers.clear();
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn read_frame(reader: &mut impl BufRead) -> Result<Value, String> {
    let mut line = String::new();
    let bytes = reader
        .take(4096)
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    if bytes == 0 {
        return Err("bridge channel closed".to_owned());
    }
    serde_json::from_str(&line).map_err(|_| "bridge returned invalid JSON".to_owned())
}

fn send_frame(writer: &mut impl Write, value: Value) -> Result<(), String> {
    writeln!(writer, "{value}")
        .and_then(|_| writer.flush())
        .map_err(|error| error.to_string())
}

fn run_route(
    route: &WorkspaceRemoteRoute,
    bridge_id: &str,
    alive: Arc<AtomicBool>,
    stop: &WorkerStop,
    context: &RouteContext<'_>,
) -> Result<(), String> {
    let forward = route
        .client
        .start_reverse_workspace_forward(context.port)
        .map_err(|error| error.to_string())?;
    if stop.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let process = route
        .client
        .open_workspace_bridge(&route.helper_path)
        .map_err(|error| error.to_string())?;
    let (reader, mut writer, shutdown) = process.into_parts();
    stop.install(shutdown);
    if stop.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let herdr_socket = route
        .client
        .herdr_socket_path()
        .map_err(|error| error.to_string())?;
    send_frame(
        &mut writer,
        json!({
            "bridge_dir":context.bridge_dir, "herdr_socket":herdr_socket,
            "port":forward.remote_port(), "origin_port":context.port,
        }),
    )?;
    let mut reader = BufReader::new(reader);
    let ready = read_frame(&mut reader)?;
    if ready["type"] != "ready" || ready["socket"].as_str().is_none() {
        return Err("remote bridge did not become ready".to_owned());
    }
    eprintln!(
        "{}",
        json!({"component":"workspace_bridge","kind":"route.ready","device_id":route.device_id,"generation":route.generation,"remote_port":forward.remote_port()})
    );
    loop {
        if stop.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(reason) = forward.failure() {
            return Err(format!("reverse forward failed: {reason}"));
        }
        let frame = read_frame(&mut reader)?;
        if frame["type"] != "attest" {
            return Err("unexpected remote bridge frame".to_owned());
        }
        let id = frame["id"].as_u64().ok_or("attestation has no id")?;
        let response = match issue_from_frame(
            route,
            bridge_id,
            Arc::clone(&alive),
            context.core,
            context.registry,
            &frame,
        ) {
            Ok(token) => json!({"id":id,"ok":true,"token":token}),
            Err(reason) => json!({"id":id,"ok":false,"reason":reason}),
        };
        send_frame(&mut writer, response)?;
    }
}

fn issue_from_frame(
    route: &WorkspaceRemoteRoute,
    bridge_id: &str,
    alive: Arc<AtomicBool>,
    core: &CoreHandle,
    registry: &Registry,
    frame: &Value,
) -> Result<String, &'static str> {
    let pane_id = frame["pane_id"].as_str().ok_or("invalid_request")?;
    let identity = PaneIdentity {
        terminal_id: frame["terminal_id"]
            .as_str()
            .ok_or("invalid_request")?
            .to_owned(),
        shell_pid: frame["shell_pid"]
            .as_i64()
            .and_then(|pid| i32::try_from(pid).ok())
            .ok_or("invalid_request")?,
        shell_started: frame["shell_started"].as_u64().ok_or("invalid_request")?,
    };
    let actual = route
        .client
        .workspace_pane_identity(&route.helper_path, pane_id)
        .map_err(|_| "remote_unavailable")?;
    if actual.terminal_id != identity.terminal_id
        || actual.shell_pid != identity.shell_pid
        || actual.shell_started != identity.shell_started
    {
        return Err("pane_changed");
    }
    let attestation = pane_auth::attest_remote(core, &route.device_id, pane_id, &identity)?;
    registry.issue_remote(
        &attestation,
        pane_auth::RemoteGrant {
            bridge_id: bridge_id.to_owned(),
            alive,
            client: Arc::clone(&route.client),
            helper_path: route.helper_path.clone(),
            source_pane_id: pane_id.to_owned(),
            one_shot: frame["one_shot"] == true,
        },
    )
}
