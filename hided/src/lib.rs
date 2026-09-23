pub mod attachments;
pub mod boundary;
pub mod cli;
pub mod coexist;
pub mod core;
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
    };
    let boundary = boundary::Boundary::new(&env.home)?;
    let core = CoreHandle::spawn(options)?;
    let watch = Arc::new(watch::WatchService::new());
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
    let app = AppState {
        core: Arc::new(core),
        boundary: Arc::new(boundary),
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
    };
    let env_state_dir = env.state_dir.clone();
    // Seeded before the server accepts a client, so an Explorer event can
    // never arrive at a boundary that holds no root yet; the core has already
    // loaded its registrations by the time `CoreHandle::spawn` returns.
    refresh_roots(&app.core, &app.boundary, &app.watch, &app.index);
    spawn_root_refresh(
        Arc::clone(&app.core),
        Arc::clone(&app.boundary),
        Arc::clone(&app.watch),
        Arc::clone(&app.index),
    );
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

/// Keeps the boundary's checkout roots in step with the core: one snapshot read
/// per change notification, and the checkouts in it replace the root set
/// wholesale.
///
/// The read runs here rather than in a client's event, so no client waits on it
/// and nothing runs under the runtime mutex. The first read seeds the roots
/// in `start_daemon` before the server serves.
///
/// The notification the snapshot stream already consumes is the only trigger,
/// so the roots change exactly when a snapshot can change and nothing here runs
/// on a clock (`docs/PERFORMANCE_TESTING.md`). The reads a single burst asks for
/// beyond the first are drained before it, and the ones that do run take a
/// snapshot that already carries every change the burst announced, so a repeat
/// read converges on the same root set instead of piling up work.
fn spawn_root_refresh(
    core: Arc<CoreHandle>,
    boundary: Arc<Boundary>,
    watch: Arc<watch::WatchService>,
    index: Arc<IndexService>,
) {
    let mut changes = core.notify.subscribe();
    tokio::spawn(async move {
        // Reading with a cursor keeps the whole rest/editor/changes payload off
        // the notify path: a delta that changed only terminal chunks carries no
        // `rest`, and roots and expanded folders both live in it.
        let mut have_revision = 0u64;
        let mut have_sequence = 0u64;
        loop {
            match changes.recv().await {
                Ok(()) => {}
                // This reader fell behind; the next read catches it up. Only a
                // closed channel means the daemon is done.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
            while changes.try_recv().is_ok() {}
            let Ok(reply) = core.snapshot(have_revision, have_sequence) else {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "hided",
                        "kind": "boundary.roots_failed",
                        "message": "core owner thread is gone",
                    })
                );
                return;
            };
            if reply.bytes.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_slice::<Value>(&reply.bytes) else {
                continue;
            };
            if let Some(revision) = value.get("revision").and_then(Value::as_u64) {
                have_revision = revision;
            }
            // Advancing the terminal cursor keeps the retained chunk window out
            // of every read this reader makes: it uses none of those bytes.
            if let Some(sequence) = value.get("terminal_sequence").and_then(Value::as_u64) {
                have_sequence = sequence;
            }
            if !carries_roots(&value) {
                continue;
            }
            apply_snapshot(&value, &boundary, &watch, &index);
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
    boundary: &Boundary,
    watch: &watch::WatchService,
    index: &IndexService,
) {
    let roots = roots_from_value(value);
    index.set_roots(
        &roots
            .iter()
            .map(|root| root.path.clone())
            .collect::<Vec<_>>(),
    );
    boundary.set_roots(roots);
    let (root, expanded) = watch_state_from_value(value);
    let root = root.filter(|root| boundary.known_root(root).is_some());
    let expanded = expanded
        .into_iter()
        .filter(|path| boundary.resolve_target(path).is_ok())
        .collect();
    watch.reconcile(root, expanded);
}

fn refresh_roots(
    core: &CoreHandle,
    boundary: &Boundary,
    watch: &watch::WatchService,
    index: &IndexService,
) {
    match core.snapshot(0, 0) {
        Ok(reply) => match serde_json::from_slice::<Value>(&reply.bytes) {
            Ok(value) => apply_snapshot(&value, boundary, watch, index),
            Err(error) => eprintln!(
                "{}",
                serde_json::json!({
                    "component": "hided",
                    "kind": "boundary.roots_failed",
                    "message": format!("snapshot decode: {error}"),
                })
            ),
        },
        Err(error) => eprintln!(
            "{}",
            serde_json::json!({
                "component": "hided",
                "kind": "boundary.roots_failed",
                "message": error,
            })
        ),
    }
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
