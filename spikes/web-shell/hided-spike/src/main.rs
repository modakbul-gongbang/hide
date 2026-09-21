//! Spike hided: herdr-core snapshot/dispatch over one loopback WebSocket.
//!
//! Not product code. Refuses to start without HERDR_SOCKET_PATH, and refuses
//! the operator's default socket so an inherited pane environment cannot
//! attach to the live layout.

mod core;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde_json::{Value, json};

use crate::core::CoreHandle;

const OPERATOR_SOCKET_SUFFIX: &str = ".config/herdr/herdr.sock";

#[derive(Clone)]
struct AppState {
    core: Arc<CoreHandle>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let socket_path = required_isolated_socket()?;
    let state_path = app_state_path()?;
    let bind = bind_addr();
    let options = serde_json::to_vec(&json!({
        "schema_version": 2,
        "herdr_socket_path": socket_path,
        "herdr_bin_path": std::env::var("HERDR_BIN").ok(),
        "app_state_path": state_path,
    }))
    .map_err(|error| format!("options encode failed: {error}"))?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("hided-spike")
        .build()
        .map_err(|error| format!("tokio runtime failed: {error}"))?;
    runtime.block_on(async move { serve(options, bind).await })
}

fn required_isolated_socket() -> Result<String, String> {
    let raw = match std::env::var("HERDR_SOCKET_PATH") {
        Ok(value) if !value.is_empty() => value,
        Ok(_) | Err(_) => {
            return Err(
                "hided-spike refuses to start: HERDR_SOCKET_PATH is unset. \
                 Point it at an isolated Herdr server started for this spike. \
                 The operator socket is never used as a default."
                    .to_owned(),
            );
        }
    };
    let path = PathBuf::from(&raw);
    let resolved = path.canonicalize().unwrap_or(path);
    if is_operator_socket(&resolved) {
        return Err(format!(
            "hided-spike refuses to start: HERDR_SOCKET_PATH resolves to the operator socket ({}). \
             Start a private `herdr server` with its own HERDR_SOCKET_PATH first.",
            resolved.display()
        ));
    }
    Ok(raw)
}

fn is_operator_socket(path: &Path) -> bool {
    path.to_string_lossy()
        .ends_with(OPERATOR_SOCKET_SUFFIX)
}

fn app_state_path() -> Result<String, String> {
    if let Ok(path) = std::env::var("HIDED_SPIKE_STATE_PATH") {
        if path.is_empty() {
            return Err("HIDED_SPIKE_STATE_PATH is empty".to_owned());
        }
        return Ok(path);
    }
    let dir = std::env::temp_dir().join(format!("hided-spike-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|error| format!("state dir: {error}"))?;
    Ok(dir.join("state.json").to_string_lossy().into_owned())
}

fn bind_addr() -> SocketAddr {
    let port = std::env::var("HIDED_SPIKE_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(9876);
    SocketAddr::from(([127, 0, 0, 1], port))
}

async fn serve(options: Vec<u8>, bind: SocketAddr) -> Result<(), String> {
    let state = AppState {
        core: Arc::new(CoreHandle::spawn(options)?),
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_upgrade))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|error| format!("bind {bind} failed: {error}"))?;
    eprintln!("hided-spike listening on ws://{bind}/ws");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| format!("server: {error}"))?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    "ok\n"
}

async fn ws_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| client_loop(socket, state))
}

async fn client_loop(mut socket: WebSocket, state: AppState) {
    let mut have_revision = 0_u64;
    let mut have_terminal_sequence = 0_u64;
    let mut notify = state.core.notify.subscribe();
    if send_snapshot(
        &mut socket,
        &state,
        0,
        0,
        true,
        &mut have_revision,
        &mut have_terminal_sequence,
    )
    .await
    .is_err()
    {
        return;
    }
    loop {
        tokio::select! {
            changed = notify.recv() => {
                if changed.is_err() {
                    break;
                }
                if send_snapshot(
                    &mut socket,
                    &state,
                    have_revision,
                    have_terminal_sequence,
                    false,
                    &mut have_revision,
                    &mut have_terminal_sequence,
                )
                .await
                .is_err()
                {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Err(error) = handle_client_text(&state, &text) {
                            let payload = json!({"type":"error","message": error});
                            if socket
                                .send(Message::Text(payload.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
}

fn handle_client_text(state: &AppState, text: &str) -> Result<(), String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("client json: {error}"))?;
    let event = if value.get("schema_version").is_some() && value.get("kind").is_some() {
        value
    } else if value.get("type").and_then(Value::as_str) == Some("dispatch") {
        value
            .get("event")
            .cloned()
            .ok_or_else(|| "dispatch message missing event".to_owned())?
    } else {
        return Err("expected a core event or {\"type\":\"dispatch\",\"event\":...}".to_owned());
    };
    let bytes = serde_json::to_vec(&event).map_err(|error| format!("event encode: {error}"))?;
    state.core.dispatch(bytes)
}

async fn send_snapshot(
    socket: &mut WebSocket,
    state: &AppState,
    have_revision: u64,
    have_terminal_sequence: u64,
    full: bool,
    out_revision: &mut u64,
    out_sequence: &mut u64,
) -> Result<(), ()> {
    let bytes = state
        .core
        .snapshot(have_revision, have_terminal_sequence)
        .map_err(|_| ())?;
    if bytes.is_empty() {
        return Ok(());
    }
    let payload: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if let Some(revision) = payload.get("revision").and_then(Value::as_u64) {
        *out_revision = revision;
    }
    if let Some(sequence) = payload.get("terminal_sequence").and_then(Value::as_u64) {
        *out_sequence = sequence;
    }
    let envelope = json!({
        "type": if full { "snapshot" } else { "delta" },
        "payload": payload,
    });
    socket
        .send(Message::Text(envelope.to_string().into()))
        .await
        .map_err(|_| ())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = terminate.recv() => {}
        }
        return;
    }
    #[cfg(not(unix))]
    ctrl_c.await.ok();
}
