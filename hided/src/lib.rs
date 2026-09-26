pub mod attachments;
pub mod boundary;
pub mod cli;
pub mod coexist;
pub mod core;
pub mod demand;
pub mod device_watch;
pub mod env;
pub mod index;
pub mod opener;
pub mod server;
pub mod spawn;
pub mod state_file;
pub mod watch;

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
use crate::state_file::{DaemonState, acquire_lock, new_token, remove_state, write_state};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The name of the machine this daemon runs on, which Settings names as the
/// owner of every value the daemon stores (PRD S5.5 B35); `None` when the
/// system will not say, which the page shows as unavailable rather than a guess.
fn host_name() -> Option<String> {
    #[cfg(unix)]
    {
        let mut buffer = [0u8; 256];
        // SAFETY: the buffer outlives the call and its length is passed with it.
        let result = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
        if result != 0 {
            return None;
        }
        let end = buffer
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(buffer.len());
        let name = String::from_utf8_lossy(&buffer[..end]).trim().to_owned();
        (!name.is_empty()).then_some(name)
    }
    #[cfg(not(unix))]
    {
        std::env::var("COMPUTERNAME")
            .ok()
            .filter(|name| !name.is_empty())
    }
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
}

impl RunningDaemon {
    pub fn stop(&self) {
        self.shutdown.notify_waiters();
    }
}

pub async fn run_daemon(env: Env) -> Result<(), String> {
    let state_dir = env.state_dir.clone();
    let running = start_daemon(env).await?;
    wait_shutdown(&running).await;
    drop(running);
    remove_state(&state_dir);
    Ok(())
}

pub async fn start_daemon(env: Env) -> Result<RunningDaemon, String> {
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
    let state = DaemonState {
        pid: std::process::id(),
        port,
        token: token.clone(),
        socket: env.herdr_socket_path.clone(),
        started_at,
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
        // The web shell draws separate Agent and View areas; each
        // Workspace's presentation lives in its own versioned file (S6 D-10).
        workspace_views_path: Some(
            env.state_dir
                .join("workspace-views.json")
                .display()
                .to_string(),
        ),
        // The Swift app's state at its release path, read once for the pane
        // chords the operator set there (desktop PRD follow-up, user decision
        // 2026-09-26). It follows HOME, so an isolated run reads its own.
        shortcut_import_path: Some(
            env.home
                .join("Library/Application Support/hide/state.json")
                .display()
                .to_string(),
        ),
    };
    let boundary = Arc::new(boundary::Boundary::new(&env.home)?);
    let core = Arc::new(CoreHandle::spawn(options)?);
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
    let app = AppState {
        core,
        boundary,
        roots,
        watch: Arc::clone(&watch),
        index: Arc::clone(&index),
        attachments: Arc::clone(&attachments),
        opener,
        token: Arc::new(token.clone()),
        allowed_origins: Arc::new(server::allowed_origins(port, env.vite_origin.as_deref())),
        clients: Arc::new(AtomicUsize::new(0)),
        connections: Arc::new(AtomicU64::new(0)),
        last_client_gone: Arc::new(Mutex::new(Instant::now())),
        keep_alive: env.keep_alive,
        idle_secs: env.idle_secs,
        shutdown: Arc::clone(&shutdown),
        ui_dir: if server::has_embedded_ui() {
            None
        } else {
            find_ui_dir()
        },
        version: VERSION,
        demand: Arc::new(demand::ObservationDemand::default()),
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
    };
    let env_state_dir = env.state_dir.clone();
    // Seeded before the server accepts a client with the registrations the
    // core loaded in `CoreHandle::spawn`. A checkout the core learns later,
    // from Herdr's first sync, reaches a client's snapshot and these roots on
    // separate reads; a refused event catches them up (`server::admit_event`).
    if let Err(error) = app.roots.catch_up() {
        roots_failed(&error);
    }
    spawn_root_refresh(Arc::clone(&app.roots));
    tokio::spawn(async move {
        if let Err(error) = server::serve(listener, app).await {
            eprintln!(
                "{}",
                serde_json::json!({"component":"hided","kind":"server.exit","message": error})
            );
        }
        remove_state(&env_state_dir);
    });
    Ok(RunningDaemon {
        port,
        token,
        _lock: lock,
        shutdown,
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
        }
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
fn spawn_root_refresh(roots: Arc<RootFollower>) {
    let mut changes = roots.core.notify.subscribe();
    tokio::spawn(async move {
        loop {
            match changes.recv().await {
                Ok(()) => {}
                // This reader fell behind; the next read catches it up. Only a
                // closed channel means the daemon is done.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
            while changes.try_recv().is_ok() {}
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
