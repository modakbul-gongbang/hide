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
use tokio::sync::Notify;

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
    let core = CoreHandle::spawn(options)?;
    let shutdown = Arc::new(Notify::new());
    let app = AppState {
        core: Arc::new(core),
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

#[cfg(test)]
mod tests {
    use super::env::{HOME, REGISTRY};

    #[test]
    fn env_registry_lists_home_as_required() {
        assert!(REGISTRY.iter().any(|key| key.key == HOME && key.required));
    }
}
