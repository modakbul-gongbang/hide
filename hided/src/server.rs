use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::core::CoreHandle;
use crate::state_file::{MAX_CLIENTS, SCHEMA_VERSION};

const FALLBACK_INDEX: &str = include_str!("../fallback-ui/index.html");

/// The web shell a release binary carries; empty in a debug build, which
/// reads `web/dist` from disk instead (`build.rs`).
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/ui_embed.rs"));
}

/// Whether this binary carries the web shell (a release build).
pub fn has_embedded_ui() -> bool {
    !embedded::FILES.is_empty()
}

/// The embedded file for a request path: `/` is `index.html`, anything else
/// is an exact relative path, so a traversal segment never matches a key.
pub fn embedded_file(path: &str) -> Option<(&'static str, &'static [u8])> {
    let relative = path.trim_start_matches('/');
    let name = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    embedded::FILES
        .iter()
        .find(|(file, _)| *file == name)
        .map(|(file, bytes)| (*file, *bytes))
}

#[derive(Clone)]
pub struct AppState {
    pub core: Arc<CoreHandle>,
    pub token: Arc<String>,
    pub allowed_origins: Arc<HashSet<String>>,
    pub clients: Arc<AtomicUsize>,
    pub last_client_gone: Arc<Mutex<Instant>>,
    pub keep_alive: bool,
    pub idle_secs: u64,
    pub shutdown: Arc<Notify>,
    pub ui_dir: Option<PathBuf>,
    pub version: &'static str,
}

#[derive(Debug, Deserialize)]
struct Handshake {
    token: String,
    schema_version: u32,
    have_revision: Option<u64>,
    have_terminal_sequence: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseReason {
    InvalidToken,
    OriginNotAllowed,
    SchemaMismatch,
    ClientLimit,
}

impl CloseReason {
    pub fn code(self) -> u16 {
        match self {
            Self::InvalidToken => 4001,
            Self::OriginNotAllowed => 4002,
            Self::SchemaMismatch => 4003,
            Self::ClientLimit => 4004,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::InvalidToken => "invalid_token",
            Self::OriginNotAllowed => "origin_not_allowed",
            Self::SchemaMismatch => "schema_mismatch",
            Self::ClientLimit => "client_limit",
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_upgrade))
        .route("/", get(static_asset))
        .route("/assets/{*path}", get(static_asset))
        .fallback(|| async { StatusCode::NOT_FOUND })
        .with_state(state)
}

fn idle_remaining_secs(state: &AppState) -> Option<u64> {
    if state.keep_alive {
        return None;
    }
    if state.clients.load(Ordering::SeqCst) > 0 {
        return Some(state.idle_secs);
    }
    let gone = *state.last_client_gone.lock().expect("client timestamp");
    Some(state.idle_secs.saturating_sub(gone.elapsed().as_secs()))
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    axum::Json(json!({
        "pid": std::process::id(),
        "version": state.version,
        "schema_version": SCHEMA_VERSION,
        "clients": state.clients.load(Ordering::SeqCst),
        "idle_remaining_secs": idle_remaining_secs(&state),
    }))
}

fn confined_file(root: &std::path::Path, relative: &str) -> Option<std::path::PathBuf> {
    if relative.split(['/', '\\']).any(|segment| segment == "..") {
        return None;
    }
    let candidate = if relative.is_empty() {
        root.join("index.html")
    } else {
        root.join(relative)
    };
    let root = root.canonicalize().ok()?;
    let file = candidate.canonicalize().ok()?;
    file.starts_with(&root).then_some(file)
}

async fn static_asset(uri: Uri, State(state): State<AppState>) -> Response {
    let path = uri.path();
    if has_embedded_ui() {
        return match embedded_file(path) {
            Some((name, bytes)) => Response::builder()
                .status(StatusCode::OK)
                .header("content-type", mime_for(std::path::Path::new(name)))
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
            None => StatusCode::NOT_FOUND.into_response(),
        };
    }
    if let Some(dir) = &state.ui_dir {
        let relative = path.trim_start_matches('/');
        if let Some(candidate) = confined_file(dir, relative)
            && candidate.is_file()
        {
            return match tokio::fs::read(&candidate).await {
                Ok(bytes) => Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", mime_for(&candidate))
                    .body(Body::from(bytes))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
                Err(_) => StatusCode::NOT_FOUND.into_response(),
            };
        }
        if (relative.is_empty() || !relative.contains('.'))
            && let Some(index) = confined_file(dir, "index.html")
            && let Ok(bytes) = tokio::fs::read(index).await
        {
            return Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/html; charset=utf-8")
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
    }
    if path == "/" {
        return Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/html; charset=utf-8")
            .body(Body::from(FALLBACK_INDEX))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }
    StatusCode::NOT_FOUND.into_response()
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    let origin = headers
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    ws.on_upgrade(move |socket| client_loop(socket, state, origin))
}

fn check_origin(origin: Option<&str>, allowed: &HashSet<String>) -> Result<(), CloseReason> {
    let Some(origin) = origin else {
        return Err(CloseReason::OriginNotAllowed);
    };
    if allowed.iter().any(|allowed| allowed == origin) {
        Ok(())
    } else {
        Err(CloseReason::OriginNotAllowed)
    }
}

async fn client_loop(mut socket: WebSocket, state: AppState, origin: Option<String>) {
    if let Err(reason) = check_origin(origin.as_deref(), &state.allowed_origins) {
        refuse(&mut socket, reason, None).await;
        return;
    }
    let first = match socket.recv().await {
        Some(Ok(Message::Text(text))) => text,
        _ => {
            refuse(&mut socket, CloseReason::InvalidToken, None).await;
            return;
        }
    };
    let handshake: Handshake = match serde_json::from_str(&first) {
        Ok(value) => value,
        Err(_) => {
            refuse(&mut socket, CloseReason::InvalidToken, None).await;
            return;
        }
    };
    if handshake.token != *state.token {
        refuse(&mut socket, CloseReason::InvalidToken, None).await;
        return;
    }
    if handshake.schema_version != SCHEMA_VERSION {
        refuse(&mut socket, CloseReason::SchemaMismatch, None).await;
        return;
    }
    let previous = state.clients.fetch_add(1, Ordering::SeqCst);
    if previous >= MAX_CLIENTS {
        state.clients.fetch_sub(1, Ordering::SeqCst);
        refuse(&mut socket, CloseReason::ClientLimit, Some(previous + 1)).await;
        return;
    }
    let mut have_revision = handshake.have_revision.unwrap_or(0);
    let mut have_sequence = handshake.have_terminal_sequence.unwrap_or(0);
    let mut notify = state.core.notify.subscribe();
    if send_snapshot(
        &mut socket,
        &state,
        0,
        0,
        true,
        &mut have_revision,
        &mut have_sequence,
    )
    .await
    .is_err()
    {
        client_gone(&state);
        return;
    }
    loop {
        tokio::select! {
            changed = notify.recv() => {
                match changed {
                    Ok(()) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
                if send_snapshot(
                    &mut socket,
                    &state,
                    have_revision,
                    have_sequence,
                    false,
                    &mut have_revision,
                    &mut have_sequence,
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
                            if socket.send(Message::Text(payload.to_string().into())).await.is_err() {
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
    client_gone(&state);
}

fn handle_client_text(state: &AppState, text: &str) -> Result<(), String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("client json: {error}"))?;
    let event = if value.get("schema_version").is_some() && value.get("kind").is_some() {
        value
    } else {
        return Err("expected a core event {schema_version, kind, payload}".to_owned());
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
    let snapshot = state
        .core
        .snapshot(have_revision, have_terminal_sequence)
        .map_err(|_| ())?;
    if snapshot.bytes.is_empty() {
        return Ok(());
    }
    let payload: Value = serde_json::from_slice(&snapshot.bytes).map_err(|_| ())?;
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
        .map_err(|_| ())?;
    Ok(())
}

fn client_gone(state: &AppState) {
    let remaining = state
        .clients
        .fetch_sub(1, Ordering::SeqCst)
        .saturating_sub(1);
    if remaining == 0 {
        *state.last_client_gone.lock().expect("client timestamp") = Instant::now();
    }
}

async fn refuse(socket: &mut WebSocket, reason: CloseReason, extra: Option<usize>) {
    log_refusal(reason, extra);
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: reason.code(),
            reason: reason.name().into(),
        })))
        .await;
}

fn log_refusal(reason: CloseReason, extra: Option<usize>) {
    eprintln!(
        "{}",
        json!({
            "component": "hided",
            "kind": "ws.refused",
            "reason": reason.name(),
            "code": reason.code(),
            "clients": extra,
        })
    );
}

pub async fn serve(listener: tokio::net::TcpListener, state: AppState) -> Result<(), String> {
    let idle_secs = state.idle_secs;
    let keep_alive = state.keep_alive;
    let last_client_gone = Arc::clone(&state.last_client_gone);
    let clients = Arc::clone(&state.clients);
    let shutdown = Arc::clone(&state.shutdown);
    let idle_task = {
        let shutdown = Arc::clone(&shutdown);
        tokio::spawn(async move {
            if keep_alive {
                return;
            }
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if clients.load(Ordering::SeqCst) != 0 {
                    continue;
                }
                let gone = *last_client_gone.lock().expect("client timestamp");
                if gone.elapsed() >= Duration::from_secs(idle_secs) {
                    shutdown.notify_waiters();
                    return;
                }
            }
        })
    };
    let app = router(state);
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.notified().await;
        })
        .await
        .map_err(|error| format!("server: {error}"))?;
    idle_task.abort();
    Ok(())
}

pub async fn bind(addr: SocketAddr) -> Result<tokio::net::TcpListener, String> {
    tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|error| format!("bind {addr} failed: {error}"))
}

fn mime_for(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

pub fn allowed_origins(port: u16, vite: Option<&str>) -> HashSet<String> {
    let mut set = HashSet::new();
    set.insert(format!("http://127.0.0.1:{port}"));
    set.insert(format!("http://localhost:{port}"));
    if let Some(vite) = vite {
        set.insert(vite.trim_end_matches('/').to_owned());
    }
    set
}
