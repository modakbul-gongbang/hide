pub mod agent_cli;
pub mod attachments;
pub mod boundary;
mod browser_assets;
pub mod browser_cli;
pub mod browser_control;
pub mod browser_page;
pub mod browser_relay;
pub mod browser_routes;
pub mod build_id;
pub mod cli;
pub mod core;
pub mod delivery_cli;
pub mod demand;
pub mod device_watch;
pub mod env;
pub mod factory_cli;
pub mod file_url;
pub mod index;
pub mod mobile;
pub mod opener;
pub mod pane_auth;
pub mod remote_bridge;
pub mod server;
pub mod spawn;
pub mod state_file;
// The old folder is this Mac's history; no Windows build ever wrote it.
#[cfg(unix)]
pub mod state_move;
pub mod watch;
pub mod workspace_cli;

use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use herdr_core::{CoreOptions, SCHEMA_VERSION};
use serde_json::Value;
use tokio::sync::Notify;

use crate::attachments::Attachments;
use crate::boundary::{Boundary, Root};
use crate::core::CoreHandle;
use crate::env::Env;
use crate::index::IndexService;
use crate::server::AppState;
use crate::state_file::{DaemonState, acquire_lock, forget_daemon, new_token, write_state};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The name of the machine this daemon runs on, which Settings names as the
/// owner of every value the daemon stores (PRD S5.5 B35); `None` when the
/// system will not say, which the page shows as unavailable rather than a guess.
fn host_name() -> Option<String> {
    hide_platform::host::name().ok()
}

/// Reads the operating system identity before the core is placed behind its
/// runtime mutex. Failure is explicit in the diagnostic and leaves
/// cross-device lineage unresolved rather than guessing from a host name.
fn machine_id() -> Option<String> {
    hide_platform::host::machine_id().ok()
}

fn find_ui_dir() -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("HIDED_UI_DIR") {
        let path = std::path::PathBuf::from(dir);
        if path.join("index.html").is_file() {
            return Some(path);
        }
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(std::path::Path::to_path_buf);
        for _ in 0..5 {
            let Some(current) = dir else { break };
            candidates.push(current.join("web/dist"));
            dir = current.parent().map(std::path::Path::to_path_buf);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("web/dist"));
        candidates.push(cwd.join("dist"));
        if let Some(parent) = cwd.parent() {
            candidates.push(parent.join("web/dist"));
        }
    }
    candidates
        .into_iter()
        .find(|path| path.join("index.html").is_file())
}

pub struct RunningDaemon {
    pub port: u16,
    pub token: String,
    pub _lock: std::fs::File,
    shutdown: Arc<Notify>,
    pane_capabilities: Arc<pane_auth::Registry>,
    pane_bootstrap_socket: std::path::PathBuf,
    pane_bootstrap_record: std::path::PathBuf,
    remote_bridges: Arc<remote_bridge::Supervisor>,
    pub mobile: Arc<mobile::Mobile>,
    /// Ended on drop, before the instance lock is released: the core's last
    /// layout and state saves are on disk before another daemon can start,
    /// and before whoever owns the state folder can remove it.
    core: Arc<CoreHandle>,
}

impl RunningDaemon {
    fn remove_bootstrap_socket(&self) {
        let _ = std::fs::remove_file(&self.pane_bootstrap_record);
        let _ = std::fs::remove_file(&self.pane_bootstrap_socket);
        if let Some(directory) = self.pane_bootstrap_socket.parent() {
            let _ = std::fs::remove_dir(directory);
        }
    }

    pub fn stop(&self) {
        self.remote_bridges.stop_all();
        self.pane_capabilities.revoke_all();
        self.remove_bootstrap_socket();
        self.shutdown.notify_waiters();
    }
}

impl Drop for RunningDaemon {
    fn drop(&mut self) {
        self.remote_bridges.stop_all();
        self.shutdown.notify_waiters();
        self.pane_capabilities.revoke_all();
        self.remove_bootstrap_socket();
        self.core.shutdown();
    }
}

pub async fn run_daemon(env: Env) -> Result<(), String> {
    #[cfg(windows)]
    temp_start_cwd_sampler();
    let state_dir = env.state_dir.clone();
    let running = start_daemon(env).await?;
    wait_shutdown_or_signal(&running).await;
    // A graceful stop takes hide's `tailscale serve` entry with it; a crash
    // leaves it to the next start's reconcile (PRD D-07).
    running.mobile.shutdown().await;
    drop(running);
    // Its own state only: the instance lock is released above, so a daemon
    // started since may already have written its own.
    forget_daemon(&state_dir, std::process::id());
    Ok(())
}

/// Waits for the daemon's own shutdown (idle) or for a stop request, and
/// turns either into the same graceful stop.
async fn wait_shutdown_or_signal(running: &RunningDaemon) {
    tokio::select! {
        _ = running.shutdown.notified() => {}
        () = stop_requested() => running.shutdown.notify_waiters(),
    }
}

/// Returns when the process is asked to stop: SIGTERM, which `hide stop`
/// sends, or SIGINT on Unix; Ctrl+C, Ctrl+Break or the console closing on
/// Windows. Where a handler cannot be installed it never returns, and the
/// daemon stops only on its own shutdown.
#[cfg(unix)]
async fn stop_requested() {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut terminate), Ok(mut interrupt)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
    }
}

#[cfg(windows)]
async fn stop_requested() {
    use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close};
    let (Ok(mut interrupt), Ok(mut break_key), Ok(mut close)) =
        (ctrl_c(), ctrl_break(), ctrl_close())
    else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = interrupt.recv() => {}
        _ = break_key.recv() => {}
        _ = close.recv() => {}
    }
}

pub async fn start_daemon(env: Env) -> Result<RunningDaemon, String> {
    // Refused before anything is written: a daemon that came up on this
    // path would answer healthy and then fail every pane attach in turn.
    if let Some(error) = env::herdr_bin_error(&env) {
        return Err(error);
    }
    let lock = acquire_lock(&env.state_dir).map_err(|error| error.to_string())?;
    let host_id = state_file::host_id(&env.state_dir)
        .map_err(|error| format!("the daemon host id could not be read or written: {error}"))?;
    let token = new_token();
    let listener = server::bind(env.bind).await?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let started_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into());
    let pid = std::process::id();
    let state = DaemonState {
        pid,
        port,
        token: token.clone(),
        socket: env.herdr_socket_path.clone(),
        started_at,
        pid_started: hide_platform::process::start_time(pid).ok(),
    };
    write_state(&env.state_dir, &state).map_err(|error| error.to_string())?;
    match env.herdr_bin_path.as_ref() {
        Some(path) => eprintln!(
            "{}",
            serde_json::json!({
                "component": "hided",
                "kind": "herdr_bin.resolved",
                "path": path.display().to_string(),
            })
        ),
        None => eprintln!(
            "{}",
            serde_json::json!({
                "component": "hided",
                "kind": "herdr_bin.missing",
                "message": "no herdr binary: HERDR_BIN_PATH is unset and PATH has none; pane terminals cannot attach",
            })
        ),
    }
    let options = CoreOptions {
        schema_version: SCHEMA_VERSION,
        // A relative HOME names no account folder; the core then reports
        // HOME invalid and turns off what needs it, as it always has.
        home: env
            .home
            .is_absolute()
            .then(|| env.home.display().to_string()),
        machine_id: machine_id(),
        herdr_socket_path: env.herdr_socket_path.clone(),
        herdr_bin_path: env
            .herdr_bin_path
            .as_ref()
            .map(|path| path.display().to_string()),
        app_state_path: env.state_dir.join("core-state.json").display().to_string(),
        // The device helper packages ship beside this binary.
        host_helper_dir: std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.display().to_string())),
        host_helper_root: env.host_helper_root.clone(),
        host_cli_dir: env.host_cli_dir.clone(),
        // The web shell draws separate Agent and View areas; each
        // Workspace's presentation lives in its own versioned file (S6 D-10).
        workspace_views_path: Some(
            env.state_dir
                .join("workspace-views.json")
                .display()
                .to_string(),
        ),
        // The removed native app's state at its release path, read once for the pane
        // chords the operator set there (desktop PRD follow-up, user decision
        // 2026-09-26). It follows HOME, so an isolated run reads its own. The
        // app only ever ran on macOS.
        shortcut_import_path: cfg!(target_os = "macos").then(|| {
            env.home
                .join("Library/Application Support/hide/state.json")
                .display()
                .to_string()
        }),
        local_issues_path: Some(
            env.state_dir
                .join("local-issues.json")
                .display()
                .to_string(),
        ),
        // The install kit's parts ship beside this binary in the app bundle;
        // a daemon anywhere else installs nothing (PRD device-parity D-19).
        kit_dir: std::env::current_exe()
            .ok()
            .as_deref()
            .and_then(hide_kit::bundled_kit_dir)
            .map(|dir| dir.display().to_string()),
    };
    let boundary = Arc::new(boundary::Boundary::new(&env.home)?);
    let core = Arc::new(CoreHandle::spawn(options)?);
    // After the core installed the diagnostic log beside its state.
    #[cfg(unix)]
    state_move::log_left_behind(env.legacy_state_dir.as_deref(), &env.state_dir);
    let pane_capabilities = Arc::new(pane_auth::Registry::new(&env.state_dir)?);
    let remote_bridges = remote_bridge::Supervisor::spawn(
        Arc::clone(&core),
        Arc::clone(&pane_capabilities),
        port,
        env.workspace_bridge_dir.clone(),
    );
    let (pane_listener, pane_bootstrap_socket) = pane_auth::bind(&env.state_dir)?;
    let watch = Arc::new(watch::WatchService::new(
        Arc::clone(&boundary),
        Arc::clone(&core),
    ));
    let index = Arc::new(IndexService::new());
    let attachments = Arc::new(Attachments::new(&env.state_dir));
    let shutdown = Arc::new(Notify::new());
    let supervisor_exe = std::env::current_exe()
        .map_err(|error| format!("cannot resolve opener supervisor executable: {error}"))?;
    let opener = opener::OpenHandler::new(
        env.open_command.clone(),
        Arc::clone(&shutdown),
        supervisor_exe,
    );
    let roots = Arc::new(RootFollower::new(
        Arc::clone(&core),
        Arc::clone(&boundary),
        Arc::clone(&watch),
        Arc::clone(&index),
    ));
    let browser_routes = browser_routes::BrowserRoutes::new(Arc::clone(&core));
    let desktop_renderers = Arc::new(AtomicUsize::new(0));
    browser_routes.spawn_reaper(Arc::clone(&desktop_renderers), Arc::clone(&shutdown));
    let renderers = Arc::new(AtomicUsize::new(0));
    let start_demand = Arc::new(demand::ObservationDemand::default());
    let mobile = mobile::Mobile::start(mobile::Config {
        state_dir: env.state_dir.clone(),
        home: env.home.clone(),
        port,
        cli: mobile::tailscale::CliSource {
            pinned: env.tailscale_bin.clone(),
            search_path: env.search_path.clone(),
        },
        host_name: host_name(),
        core: Arc::clone(&core),
        herdr_socket: env.herdr_socket_path.as_ref().map(std::path::PathBuf::from),
        renderers: Arc::clone(&renderers),
        start_demand: Arc::clone(&start_demand),
    });
    let app = AppState {
        core: Arc::clone(&core),
        boundary,
        roots,
        watch: Arc::clone(&watch),
        index: Arc::clone(&index),
        attachments: Arc::clone(&attachments),
        opener,
        token: Arc::new(token.clone()),
        pane_capabilities: Arc::clone(&pane_capabilities),
        browser_routes,
        browser_control: Arc::new(browser_control::BrowserControl::default()),
        herdr_socket: env.herdr_socket_path.as_ref().map(std::path::PathBuf::from),
        allowed_origins: Arc::new(server::allowed_origins(port, env.vite_origin.as_deref())),
        clients: Arc::new(AtomicUsize::new(0)),
        renderers,
        desktop_renderers,
        connections: Arc::new(AtomicU64::new(0)),
        last_client_gone: Arc::new(Mutex::new(Instant::now())),
        keep_alive: env.keep_alive,
        idle_secs: env.idle_secs,
        build: env.build.as_deref().map(Arc::from),
        renderer_transitions: Arc::new(Mutex::new(())),
        shutdown: Arc::clone(&shutdown),
        ui_dir: if server::has_embedded_ui() {
            None
        } else {
            find_ui_dir()
        },
        version: VERSION,
        demand: Arc::new(demand::ObservationDemand::default()),
        start_demand,
        request_view_demand: Arc::new(demand::ObservationDemand::default()),
        daemon_info: Arc::new(serde_json::json!({
            "version": VERSION,
            "schema_version": SCHEMA_VERSION,
            "host_id": host_id,
            "host_name": host_name(),
            "pid": std::process::id(),
            "started_at_unix": state.started_at.clone(),
            "state_dir": env.state_dir.display().to_string(),
            "core_state_path": env.state_dir.join("core-state.json").display().to_string(),
            "herdr_bin_path": env.herdr_bin_path.as_ref().map(|path| path.display().to_string()),
            "herdr_socket_path": env.herdr_socket_path.clone(),
            "keep_alive": env.keep_alive,
            "idle_secs": env.idle_secs,
        })),
        mobile: Arc::clone(&mobile),
    };
    let env_state_dir = env.state_dir.clone();
    // Seeded before the server accepts a client with the registrations the
    // core loaded in `CoreHandle::spawn`. A checkout the core learns later,
    // from Herdr's first sync, reaches a client's snapshot and these roots on
    // separate reads; a refused event catches them up (`server::admit_event`).
    if let Err(error) = app.roots.catch_up() {
        roots_failed(&error);
    }
    spawn_root_refresh(Arc::clone(&app.roots), Arc::clone(&app.clients));
    // The core starts out drawn; until a window says otherwise, it is not.
    server::dispatch_ui_attached(&app.core, false);
    tokio::spawn(pane_auth::serve(
        pane_listener,
        Arc::clone(&pane_capabilities),
        Arc::clone(&core),
        env.herdr_socket_path.as_ref().map(std::path::PathBuf::from),
        port,
        Arc::clone(&shutdown),
    ));
    tokio::spawn(async move {
        if let Err(error) = server::serve(listener, app).await {
            eprintln!(
                "{}",
                serde_json::json!({"component":"hided","kind":"server.exit","message": error})
            );
        }
        forget_daemon(&env_state_dir, std::process::id());
    });
    Ok(RunningDaemon {
        port,
        token,
        _lock: lock,
        shutdown,
        pane_capabilities,
        pane_bootstrap_socket,
        pane_bootstrap_record: pane_auth::bootstrap_socket_record(&env.state_dir),
        remote_bridges,
        mobile,
        core,
    })
}

pub async fn wait_shutdown(running: &RunningDaemon) {
    running.shutdown.notified().await;
}

/// Follows the core's checkout roots into the boundary, the watch service and
/// the index: each read takes what the core changed since the last one, and
/// the checkouts in it replace the root set wholesale.
///
/// Three readers share it: the seed in `start_daemon` before the server
/// serves, the notification reader (`spawn_root_refresh`), and a client event
/// the boundary refused as outside every checkout (`server::admit_event`). One
/// pair of cursors behind one lock means each read starts where the last one
/// ended, a reader never applies an older snapshot over a newer one, and a
/// read that waited for another to finish finds the roots that one applied.
///
/// Reading with a cursor keeps the whole rest/editor/changes payload off the
/// notify path: a delta that changed only terminal chunks carries no `rest`,
/// and roots and expanded folders both live in it. Nothing here runs under
/// the runtime mutex.
pub struct RootFollower {
    core: Arc<CoreHandle>,
    boundary: Arc<Boundary>,
    watch: Arc<watch::WatchService>,
    index: Arc<IndexService>,
    /// The revision and terminal sequence the last read reached.
    cursors: Mutex<(u64, u64)>,
    /// Wakes the refresh loop when a client arrives after none were
    /// connected, so the reads it skipped meanwhile catch up at once.
    resumed: Notify,
}

impl RootFollower {
    pub fn new(
        core: Arc<CoreHandle>,
        boundary: Arc<Boundary>,
        watch: Arc<watch::WatchService>,
        index: Arc<IndexService>,
    ) -> Self {
        Self {
            core,
            boundary,
            watch,
            index,
            cursors: Mutex::new((0, 0)),
            resumed: Notify::new(),
        }
    }

    /// A client arrived after none were connected.
    pub fn resume(&self) {
        self.resumed.notify_one();
    }

    /// Reads what the core changed since the last read and applies the roots
    /// in it; true when the read carried them. An error means the core's
    /// owner thread is gone.
    pub fn catch_up(&self) -> Result<bool, String> {
        let mut cursors = self
            .cursors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let reply = self.core.snapshot(cursors.0, cursors.1)?;
        if reply.bytes.is_empty() {
            return Ok(false);
        }
        let value = match serde_json::from_slice::<Value>(&reply.bytes) {
            Ok(value) => value,
            Err(error) => {
                roots_failed(&format!("snapshot decode: {error}"));
                return Ok(false);
            }
        };
        if let Some(revision) = value.get("revision").and_then(Value::as_u64) {
            cursors.0 = revision;
        }
        // Advancing the terminal cursor keeps the retained chunk window out of
        // every later read: this reader uses none of those bytes.
        if let Some(sequence) = value.get("terminal_sequence").and_then(Value::as_u64) {
            cursors.1 = sequence;
        }
        if !carries_roots(&value) {
            return Ok(false);
        }
        apply_snapshot(&value, &self.core, &self.boundary, &self.watch, &self.index);
        Ok(true)
    }
}

/// The one line a failed root read leaves.
pub(crate) fn roots_failed(message: &str) {
    eprintln!(
        "{}",
        serde_json::json!({
            "component": "hided",
            "kind": "boundary.roots_failed",
            "message": message,
        })
    );
}

/// Keeps the boundary's checkout roots in step with the core: one read per
/// change notification.
///
/// The notification the snapshot stream already consumes is the only trigger,
/// so the roots change exactly when a snapshot can change and nothing here runs
/// on a clock (`docs/PERFORMANCE_TESTING.md`). The reads a single burst asks for
/// beyond the first are drained before it, and the ones that do run take a
/// snapshot that already carries every change the burst announced, so a repeat
/// read converges on the same root set instead of piling up work.
///
/// With no client connected nothing reads the roots but a refused event,
/// which catches them up itself (`server::admit_event`), so the reads rest
/// until a client arrives and one read then covers everything skipped (PRD
/// labels-in-hided B29): the core changes on every label while no window is
/// open, and a snapshot per change would be the daemon's largest cost then.
fn spawn_root_refresh(roots: Arc<RootFollower>, clients: Arc<AtomicUsize>) {
    let mut changes = roots.core.notify.subscribe();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                change = changes.recv() => match change {
                    Ok(()) => {}
                    // This reader fell behind; the next read catches it up.
                    // Only a closed channel means the daemon is done.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                },
                () = roots.resumed.notified() => {}
            }
            while changes.try_recv().is_ok() {}
            if clients.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                continue;
            }
            if let Err(error) = roots.catch_up() {
                roots_failed(&error);
                return;
            }
        }
    });
}

/// Whether a snapshot value carries the sections this reader needs: a `rest`
/// object with a navigator. A delta that changed only terminal chunks carries
/// no `rest` at all, or a null, and reading it would clear the roots.
fn carries_roots(value: &Value) -> bool {
    value.get("rest").is_some_and(Value::is_object)
        && value.pointer("/rest/navigator/workspaces").is_some()
}

/// One snapshot value's roots, watch folders and registered index roots.
fn apply_snapshot(
    value: &Value,
    core: &CoreHandle,
    boundary: &Boundary,
    watch: &watch::WatchService,
    index: &IndexService,
) {
    let roots = roots_from_value(value);
    boundary.set_roots(roots.clone());
    boundary.set_device_roots(device_roots_from_value(value));
    if let Err(error) = core.set_file_roots(boundary.opened_roots()) {
        eprintln!(
            "{}",
            serde_json::json!({
                "component": "hided", "kind": "boundary.core_roots_failed", "message": error
            })
        );
    }
    let device_roots = device_roots_from_value(value);
    index.set_roots(
        &roots
            .iter()
            .map(|root| {
                (
                    herdr_core::workspace::LOCAL_DEVICE_ID.to_owned(),
                    root.path.display().to_string(),
                )
            })
            .chain(
                device_roots
                    .iter()
                    .map(|root| (root.device_id.clone(), root.path.clone())),
            )
            .collect::<Vec<_>>(),
    );
    let (root, expanded) = watch_state_from_value(value);
    let root = root.filter(|root| boundary.known_root(root).is_some());
    let expanded = expanded
        .into_iter()
        .filter(|path| boundary.resolve_target(path).is_ok())
        .collect();
    watch.reconcile(boundary, root, expanded);
    watch.reconcile_device(device_watch::target_from_value(value));
}

/// The folders whose changes the Explorer wants announced: the focused
/// checkout's root and the folders the core reports as expanded. The watch
/// cap is applied by `watch::watched_folders`, not here, so the two sides of
/// the protocol read the same rule from one place.
fn watch_state_from_value(value: &Value) -> (Option<String>, Vec<String>) {
    let root = value
        .pointer("/rest/navigator/root_path")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let expanded = value
        .pointer("/rest/ui_state/expanded_paths")
        .and_then(Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    (root, expanded)
}

/// The checkouts a snapshot carries that the Explorer may work in: every local
/// workspace, and the checkouts in it that exist on disk. A remote workspace's
/// paths name another machine and are not this boundary's to read. The tests
/// read it from bytes; production reads the value the notify path already
/// decoded.
#[cfg(test)]
fn roots_from_snapshot(bytes: &[u8]) -> Vec<Root> {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return Vec::new();
    };
    roots_from_value(&value)
}

fn roots_from_value(value: &Value) -> Vec<Root> {
    let Some(workspaces) = value
        .pointer("/rest/navigator/workspaces")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    for workspace in workspaces {
        if workspace
            .get("remote_target_id")
            .is_some_and(|target| !target.is_null())
        {
            continue;
        }
        let Some(workspace_id) = workspace.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(checkouts) = workspace.get("checkouts").and_then(Value::as_array) else {
            continue;
        };
        for checkout in checkouts {
            let Some(id) = checkout.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(path) = checkout.get("path").and_then(Value::as_str) else {
                continue;
            };
            if !checkout
                .get("exists")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            roots.push(Root {
                workspace_id: workspace_id.to_owned(),
                checkout_id: id.to_owned(),
                path: std::path::PathBuf::from(path),
            });
        }
    }
    roots
}

/// The checkouts a snapshot carries on SSH devices: the device and the root
/// path there, which only that device's helper reads. A device's projects
/// are the registered ones in the navigator and its Herdr session's, the
/// same two places the core's `catalog_checkout` looks.
fn device_roots_from_value(value: &Value) -> Vec<boundary::DeviceRoot> {
    let registered = value
        .pointer("/rest/navigator/workspaces")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    let sessions = value
        .pointer("/rest/status/remote")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|remote| {
            remote
                .pointer("/session/workspaces")
                .and_then(Value::as_array)
        })
        .flatten();
    let mut roots = Vec::new();
    for workspace in registered.chain(sessions) {
        let Some(device_id) = workspace
            .get("device_id")
            .and_then(Value::as_str)
            .filter(|device| *device != herdr_core::workspace::LOCAL_DEVICE_ID)
        else {
            continue;
        };
        for checkout in workspace
            .get("checkouts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(path) = checkout.get("path").and_then(Value::as_str) {
                roots.push(boundary::DeviceRoot {
                    device_id: device_id.to_owned(),
                    path: path.to_owned(),
                });
            }
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::env::{HOME, REGISTRY};
    use super::*;

    #[test]
    fn only_a_snapshot_with_a_navigator_can_move_the_roots() {
        let full = serde_json::json!({"revision": 3, "rest": {"navigator": {"root_path": "/repo", "workspaces": []}}});
        assert!(carries_roots(&full));
        // A delta that changed only terminal chunks: no rest, or a null, must
        // never be read as an empty root set.
        assert!(!carries_roots(&serde_json::json!({"revision": 4})));
        assert!(!carries_roots(
            &serde_json::json!({"revision": 4, "rest": serde_json::Value::Null})
        ));
        assert!(!carries_roots(
            &serde_json::json!({"revision": 5, "rest": {"status": {}}})
        ));
    }

    #[test]
    fn env_registry_lists_home_as_required() {
        assert!(REGISTRY.iter().any(|key| key.key == HOME && key.required));
    }

    #[test]
    fn roots_come_from_the_local_checkouts_a_snapshot_carries() {
        let snapshot = serde_json::json!({
            "schema_version": 2,
            "revision": 4,
            "rest": {
                "navigator": {
                    "workspaces": [
                        {
                            "id": "w1",
                            "remote_target_id": null,
                            "checkouts": [
                                {"id": "c1", "path": "/tmp/repo", "exists": true},
                                {"id": "c2", "path": "/tmp/gone", "exists": false},
                            ],
                        },
                        {
                            "id": "w2",
                            "remote_target_id": "device-1",
                            "checkouts": [{"id": "c3", "path": "/srv/repo", "exists": true}],
                        },
                    ],
                },
            },
        });
        let bytes = serde_json::to_vec(&snapshot).unwrap();
        assert_eq!(
            roots_from_snapshot(&bytes),
            vec![Root {
                workspace_id: "w1".to_owned(),
                checkout_id: "c1".to_owned(),
                path: std::path::PathBuf::from("/tmp/repo"),
            }],
            "a remote workspace and a checkout that is gone are not roots"
        );
    }

    #[test]
    fn a_snapshot_without_rest_or_checkouts_yields_no_roots() {
        for snapshot in [
            serde_json::json!({"schema_version": 2, "revision": 1}),
            serde_json::json!({"revision": 1, "rest": {"navigator": {"workspaces": []}}}),
        ] {
            let bytes = serde_json::to_vec(&snapshot).unwrap();
            assert!(roots_from_snapshot(&bytes).is_empty());
        }
        assert!(roots_from_snapshot(b"not json").is_empty());
    }
}

/// TEMP evidence for issue 707 (do not merge): every 25 ms, the processes
/// whose working directory is inside an e2e fixture folder, logged when each
/// first appears there and when it leaves.
#[cfg(windows)]
fn temp_start_cwd_sampler() {
    let _ = std::thread::Builder::new().name("temp-cwd-sampler".into()).spawn(|| {
        let now_ms = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis())
                .unwrap_or(0)
        };
        let mut seen: std::collections::BTreeMap<(u32, String), (u32, String)> = Default::default();
        loop {
            let mut current = std::collections::BTreeMap::new();
            for (pid, parent, name, cwd) in hide_platform::process::temp_process_cwds() {
                let Some(cwd) = cwd else { continue };
                let cwd = cwd.to_string_lossy().into_owned();
                if cwd.to_lowercase().contains("hide-e2e-herdr-") {
                    current.insert((pid, cwd), (parent, name));
                }
            }
            for (key, (parent, name)) in &current {
                if !seen.contains_key(key) {
                    eprintln!("{}", serde_json::json!({"kind": "temp.cwd_seen", "pid": key.0, "parent": parent, "name": name, "cwd": key.1, "at_ms": now_ms(), "self_pid": std::process::id()}));
                }
            }
            for (key, (_, name)) in &seen {
                if !current.contains_key(key) {
                    eprintln!("{}", serde_json::json!({"kind": "temp.cwd_gone", "pid": key.0, "name": name, "cwd": key.1, "at_ms": now_ms()}));
                }
            }
            seen = current;
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    });
}
