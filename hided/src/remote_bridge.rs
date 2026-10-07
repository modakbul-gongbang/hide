//! SSH device return routes for pane-scoped Workspace commands. The core
//! selects consented device/helper pairs; this supervisor owns one reverse
//! loopback forward and one attesting helper exec channel for each pair.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use herdr_core::WorkspaceRemoteRoute;
use hide_host::pane_peer::PaneIdentity;
use hide_node::ssh::{RusshRemoteClient, SshDevice};
use serde_json::{Value, json};

use crate::core::CoreHandle;
use crate::pane_auth::{self, Registry};
use crate::state_file::new_token;

const MAX_ROUTES: usize = 8;
const POLL: Duration = Duration::from_secs(2);
/// A route that fails is tried again after this, doubling per failure up to
/// `RETRY_MAX`, so an unreachable device costs two SSH connections a minute
/// rather than every poll. A new connection generation, or a route that
/// reached ready before it failed, starts the schedule over.
const RETRY_BASE: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(60);

type Shutdown = Box<dyn FnOnce() + Send>;

/// The SSH client behind a route's device transport, for the forwards and
/// helper execs this daemon opens itself. Every device this daemon builds is
/// an `SshDevice`, so `None` means a transport this daemon did not build.
pub(crate) fn ssh_client(route: &WorkspaceRemoteRoute) -> Option<Arc<RusshRemoteClient>> {
    Arc::clone(&route.transport)
        .into_any()
        .downcast::<SshDevice>()
        .ok()
        .map(|device| Arc::clone(device.client()))
}

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

    /// Callable from the supervisor's async task: the installed shutdown
    /// ends the bridge's SSH connection without blocking on its runtime.
    fn stop(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Ok(mut slot) = self.shutdown.lock()
            && let Some(shutdown) = slot.take()
        {
            shutdown();
        }
    }
}

/// How a worker's route ended, which decides when it is tried again.
enum RouteEnd {
    Stopped,
    Failed { was_ready: bool },
}

struct Worker {
    generation: u64,
    bridge_id: String,
    stop: Arc<WorkerStop>,
    thread: JoinHandle<RouteEnd>,
}

struct Retry {
    generation: u64,
    failures: u32,
    not_before: Instant,
}

#[derive(Default)]
struct State {
    workers: HashMap<String, Worker>,
    retries: HashMap<String, Retry>,
    /// The generation each device over `MAX_ROUTES` was reported for.
    unserved: HashMap<String, u64>,
    routes_unavailable: bool,
}

/// Keeps exactly one live return route per connected, consented device.
/// Every start, stop, failure and refusal is a `workspace_bridge` record in
/// the diagnostics log, because a detached daemon's stderr goes nowhere.
pub struct Supervisor {
    closed: AtomicBool,
    state: Mutex<State>,
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
            state: Mutex::new(State::default()),
        });
        let running = Arc::clone(&supervisor);
        let task = tokio::spawn(async move {
            let mut poll = tokio::time::interval(POLL);
            loop {
                poll.tick().await;
                if running.closed.load(Ordering::Acquire) {
                    return;
                }
                let query_core = Arc::clone(&core);
                let routes =
                    match tokio::task::spawn_blocking(move || query_core.workspace_remote_routes())
                        .await
                    {
                        Ok(routes) => routes,
                        Err(error) => Err(error.to_string()),
                    };
                running.reconcile(routes, &core, &registry, port, &bridge_dir);
            }
        });
        // The loop returns only once the daemon stops. Any other end, a
        // panic included, leaves every device without a route, so it is
        // recorded rather than lost with the task.
        let closed = Arc::clone(&supervisor);
        tokio::spawn(async move {
            let reason = match task.await {
                Ok(()) if closed.closed.load(Ordering::Acquire) => "daemon_stopping".to_owned(),
                Ok(()) => "returned".to_owned(),
                Err(error) if error.is_panic() => "panicked".to_owned(),
                Err(error) => error.to_string(),
            };
            herdr_core::diagnostic!(json!({
                "component":"workspace_bridge","kind":"supervisor.stopped","reason":reason,
            }));
        });
        supervisor
    }

    fn reconcile(
        &self,
        routes: Result<Vec<WorkspaceRemoteRoute>, String>,
        core: &Arc<CoreHandle>,
        registry: &Arc<Registry>,
        port: u16,
        bridge_dir: &Option<PathBuf>,
    ) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        // A panic elsewhere must not leave every later poll locked out.
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let routes = match routes {
            Ok(routes) => {
                if std::mem::take(&mut state.routes_unavailable) {
                    herdr_core::diagnostic!(json!({
                        "component":"workspace_bridge","kind":"routes.available",
                    }));
                }
                routes
            }
            Err(reason) => {
                if !std::mem::replace(&mut state.routes_unavailable, true) {
                    herdr_core::diagnostic!(json!({
                        "component":"workspace_bridge","kind":"routes.unavailable","reason":reason,
                    }));
                }
                return;
            }
        };
        let current: HashMap<String, u64> = routes
            .iter()
            .map(|route| (route.device_id.clone(), route.generation))
            .collect();
        let now = Instant::now();
        let devices: Vec<String> = state.workers.keys().cloned().collect();
        for device in devices {
            let Some(worker) = state.workers.get(&device) else {
                continue;
            };
            let generation = current.get(&device).copied();
            if worker.thread.is_finished() {
                let Some(worker) = state.workers.remove(&device) else {
                    continue;
                };
                let was_ready = match worker.thread.join() {
                    Ok(RouteEnd::Stopped) => continue,
                    Ok(RouteEnd::Failed { was_ready }) => was_ready,
                    Err(_) => {
                        registry.revoke_bridge(&worker.bridge_id);
                        herdr_core::diagnostic!(json!({
                            "component":"workspace_bridge","kind":"route.failed",
                            "device_id":device,"generation":worker.generation,
                            "bridge_id":worker.bridge_id,"reason":"worker panicked","was_ready":false,
                        }));
                        false
                    }
                };
                schedule_retry(&mut state, &device, worker.generation, was_ready, now);
            } else if generation != Some(worker.generation) {
                let Some(worker) = state.workers.remove(&device) else {
                    continue;
                };
                herdr_core::diagnostic!(json!({
                    "component":"workspace_bridge","kind":"route.stopped",
                    "device_id":device,"generation":worker.generation,"bridge_id":worker.bridge_id,
                    "reason": if generation.is_some() { "generation_changed" } else { "route_withdrawn" },
                }));
                worker.stop.stop();
            }
        }
        state
            .retries
            .retain(|device, _| current.contains_key(device));
        state
            .unserved
            .retain(|device, _| current.contains_key(device));
        let mut served = state.workers.len();
        for route in routes {
            if state.workers.contains_key(&route.device_id) {
                continue;
            }
            if served >= MAX_ROUTES {
                if state
                    .unserved
                    .insert(route.device_id.clone(), route.generation)
                    != Some(route.generation)
                {
                    herdr_core::diagnostic!(json!({
                        "component":"workspace_bridge","kind":"route.unserved",
                        "device_id":route.device_id,"generation":route.generation,"limit":MAX_ROUTES,
                    }));
                }
                continue;
            }
            let attempt = match state.retries.get(&route.device_id) {
                Some(retry) if retry.generation == route.generation => {
                    if now < retry.not_before {
                        continue;
                    }
                    retry.failures + 1
                }
                _ => 1,
            };
            let device = route.device_id.clone();
            let generation = route.generation;
            let bridge_id = format!("{device}:{generation}:{}", new_token());
            let stop = Arc::new(WorkerStop::new());
            let spawned = {
                let core = Arc::clone(core);
                let registry = Arc::clone(registry);
                let worker_stop = Arc::clone(&stop);
                let bridge_dir = bridge_dir.clone();
                let bridge_id = bridge_id.clone();
                std::thread::Builder::new()
                    .name("hided-workspace-bridge".to_owned())
                    .spawn(move || {
                        let alive = Arc::new(AtomicBool::new(true));
                        let context = RouteContext {
                            core: &core,
                            registry: &registry,
                            port,
                            bridge_dir,
                        };
                        let mut was_ready = false;
                        let result = run_route(
                            &route,
                            &bridge_id,
                            Arc::clone(&alive),
                            &worker_stop,
                            &context,
                            &mut was_ready,
                        );
                        alive.store(false, Ordering::Release);
                        registry.revoke_bridge(&bridge_id);
                        // Stopping closes the channel, which the read loop
                        // sees as an error; that is this route's end, not a
                        // failure.
                        let stopped = worker_stop.cancelled.load(Ordering::Acquire);
                        worker_stop.stop();
                        match result {
                            Err(reason) if !stopped => {
                                herdr_core::diagnostic!(json!({
                                    "component":"workspace_bridge","kind":"route.failed",
                                    "device_id":route.device_id,"generation":route.generation,
                                    "bridge_id":bridge_id,"reason":reason,"was_ready":was_ready,
                                }));
                                RouteEnd::Failed { was_ready }
                            }
                            _ => {
                                herdr_core::diagnostic!(json!({
                                    "component":"workspace_bridge","kind":"route.closed",
                                    "device_id":route.device_id,"generation":route.generation,
                                    "bridge_id":bridge_id,
                                }));
                                RouteEnd::Stopped
                            }
                        }
                    })
            };
            match spawned {
                Ok(thread) => {
                    herdr_core::diagnostic!(json!({
                        "component":"workspace_bridge","kind":"route.starting",
                        "device_id":device,"generation":generation,"bridge_id":bridge_id,"attempt":attempt,
                    }));
                    state.workers.insert(
                        device,
                        Worker {
                            generation,
                            bridge_id,
                            stop,
                            thread,
                        },
                    );
                    served += 1;
                }
                Err(error) => {
                    herdr_core::diagnostic!(json!({
                        "component":"workspace_bridge","kind":"worker.failed",
                        "device_id":device,"generation":generation,"reason":error.to_string(),
                    }));
                    schedule_retry(&mut state, &device, generation, false, now);
                }
            }
        }
    }

    pub fn stop_all(&self) {
        self.closed.store(true, Ordering::Release);
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        for (device, worker) in state.workers.drain() {
            herdr_core::diagnostic!(json!({
                "component":"workspace_bridge","kind":"route.stopped",
                "device_id":device,"generation":worker.generation,"bridge_id":worker.bridge_id,
                "reason":"daemon_stopping",
            }));
            worker.stop.stop();
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn schedule_retry(state: &mut State, device: &str, generation: u64, was_ready: bool, now: Instant) {
    let retry = state.retries.entry(device.to_owned()).or_insert(Retry {
        generation,
        failures: 0,
        not_before: now,
    });
    if retry.generation != generation || was_ready {
        retry.generation = generation;
        retry.failures = 0;
    }
    retry.failures = retry.failures.saturating_add(1);
    retry.not_before = now + retry_delay(retry.failures);
}

fn retry_delay(failures: u32) -> Duration {
    RETRY_BASE
        .saturating_mul(1 << failures.saturating_sub(1).min(5))
        .min(RETRY_MAX)
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
    was_ready: &mut bool,
) -> Result<(), String> {
    let client = ssh_client(route).ok_or("the device transport is not SSH")?;
    let forward = client
        .start_reverse_workspace_forward(context.port)
        .map_err(|error| error.to_string())?;
    if stop.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let process = client
        .open_workspace_bridge(&route.helper_path)
        .map_err(|error| error.to_string())?;
    let (reader, mut writer, shutdown) = process.into_parts();
    stop.install(shutdown);
    if stop.cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let herdr_socket = client
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
    *was_ready = true;
    herdr_core::diagnostic!(json!({
        "component":"workspace_bridge","kind":"route.ready","device_id":route.device_id,
        "generation":route.generation,"bridge_id":bridge_id,"remote_port":forward.remote_port(),
    }));
    loop {
        if stop.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(reason) = forward.failure() {
            return Err(format!("reverse forward failed: {reason}"));
        }
        let frame = read_frame(&mut reader)?;
        if frame["type"] == "revoke" {
            if let Some(token) = frame["token"].as_str() {
                context.registry.revoke_bridge_token(bridge_id, token);
            }
            continue;
        }
        if frame["type"] == "failed" {
            return Err(format!(
                "remote bridge stopped serving: {}",
                frame["reason"].as_str().unwrap_or("no reason")
            ));
        }
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
            Ok((token, issued_new)) => {
                json!({"id":id,"ok":true,"token":token,"issued_new":issued_new})
            }
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
) -> Result<(String, bool), &'static str> {
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
    let client = ssh_client(route).ok_or("remote_unavailable")?;
    let actual = client
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
            client,
            helper_path: route.helper_path.clone(),
            source_pane_id: pane_id.to_owned(),
            one_shot: frame["one_shot"] == true,
        },
    )
}
