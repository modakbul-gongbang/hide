pub mod boundary;
pub mod cli;
pub mod coexist;
pub mod core;
pub mod env;
pub mod server;
pub mod spawn;
pub mod state_file;

use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use herdr_core::{CoreOptions, SCHEMA_VERSION};
use serde_json::Value;
use tokio::sync::Notify;

use crate::boundary::{Boundary, Root};
use crate::core::CoreHandle;
use crate::env::Env;
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
    let shutdown = Arc::new(Notify::new());
    let app = AppState {
        core: Arc::new(core),
        boundary: Arc::new(boundary),
        token: Arc::new(token.clone()),
        allowed_origins: Arc::new(server::allowed_origins(port, env.vite_origin.as_deref())),
        clients: Arc::new(AtomicUsize::new(0)),
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
    refresh_roots(&app.core, &app.boundary);
    spawn_root_refresh(Arc::clone(&app.core), Arc::clone(&app.boundary));
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
fn spawn_root_refresh(core: Arc<CoreHandle>, boundary: Arc<Boundary>) {
    let mut changes = core.notify.subscribe();
    tokio::spawn(async move {
        loop {
            if changes.recv().await.is_err() {
                return;
            }
            while changes.try_recv().is_ok() {}
            refresh_roots(&core, &boundary);
        }
    });
}

fn refresh_roots(core: &CoreHandle, boundary: &Boundary) {
    match core.snapshot(0, 0) {
        Ok(reply) => boundary.set_roots(roots_from_snapshot(&reply.bytes)),
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

/// The checkouts a snapshot carries that the Explorer may work in: every local
/// workspace, and the checkouts in it that exist on disk. A remote workspace's
/// paths name another machine and are not this boundary's to read.
fn roots_from_snapshot(bytes: &[u8]) -> Vec<Root> {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return Vec::new();
    };
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
