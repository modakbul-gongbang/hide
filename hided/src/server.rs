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

use crate::boundary::{self, Boundary, Listing, Refusal};
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
    pub boundary: Arc<Boundary>,
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
    if !token_matches(&handshake.token, &state.token) {
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
    // A reconnecting client resumes from the cursors it last applied, so the
    // first frame carries only what changed while it was away; a fresh client
    // (cursor 0) gets the whole state.
    let mut have_revision = handshake.have_revision.unwrap_or(0);
    let mut have_sequence = handshake.have_terminal_sequence.unwrap_or(0);
    let mut notify = state.core.notify.subscribe();
    if send_snapshot(&mut socket, &state, &mut have_revision, &mut have_sequence)
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
                if send_snapshot(&mut socket, &state, &mut have_revision, &mut have_sequence)
                    .await
                    .is_err()
                {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let reply = match handle_client_text(&state, &text) {
                            Ok(Some(frame)) => Some(frame),
                            Ok(None) => None,
                            Err(error) => Some(json!({"type":"error","payload":{},"message": error})),
                        };
                        if let Some(frame) = reply
                            && socket.send(Message::Text(frame.to_string().into())).await.is_err()
                        {
                            break;
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

/// Constant in the token's length, so a byte-by-byte mismatch does not leak
/// how much of the token a caller guessed.
fn token_matches(offered: &str, expected: &str) -> bool {
    use subtle::ConstantTimeEq;
    offered.len() == expected.len() && offered.as_bytes().ct_eq(expected.as_bytes()).into()
}

/// Forwards a client event to the core, or answers it here.
///
/// `Ok(Some(frame))` is a frame for this client alone: a directory listing or
/// a path refusal. Everything the core answers arrives through the snapshot
/// stream instead, so a forwarded event returns `Ok(None)`.
fn handle_client_text(state: &AppState, text: &str) -> Result<Option<Value>, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("client json: {error}"))?;
    let mut event = if value.get("schema_version").is_some() && value.get("kind").is_some() {
        value
    } else {
        return Err("expected a core event {schema_version, kind, payload}".to_owned());
    };
    if let Some(reply) = apply_boundary(&state.boundary, &mut event) {
        return Ok(Some(reply));
    }
    let bytes = serde_json::to_vec(&event).map_err(|error| format!("event encode: {error}"))?;
    state.core.dispatch(bytes).map(|()| None)
}

/// The one place a client's path is checked before the core sees it (PRD S2
/// B10, S3 D-01 and B11). Two lines run here, one per flow, and every path
/// that passes is rewritten to the spelling that was checked.
///
/// The registration line reads `$HOME`: a `remote_file_list` for the `local`
/// target or a `create_workspace` whose path does not resolve under home is
/// answered with a `path_refused` frame and never reaches the core. The local
/// listing is the web shell's directory autocomplete, which the core has no
/// event for, so hided answers it as a `directory_list` frame; a
/// `remote_file_list` for any other target names a path on that remote
/// machine, which this boundary knows nothing about, and is forwarded as it
/// came.
///
/// The Explorer line reads the registered checkout roots: every path an
/// explorer event carries is checked against the root the event named, and a
/// path outside it is refused as `outside_checkout`. `file_save` is the one
/// exception and is checked without being rewritten, because the core compares
/// the path it stored with the one it is handed.
///
/// `file_list` is the Explorer's listing: it names the root and the folder under
/// it, and the children are answered here as a `directory_list` frame, because
/// the core has no event for reading a directory and the Explorer shows files
/// the core's own listing never carries.
///
/// An attachment event carries paths the shell staged itself, so only their
/// shape is checked; a path that fails is dropped from the batch rather than
/// losing the whole event. Every other kind passes untouched.
fn apply_boundary(boundary: &Boundary, event: &mut Value) -> Option<Value> {
    let kind = event.get("kind").and_then(Value::as_str)?.to_owned();
    match kind.as_str() {
        "remote_file_list" => registration_listing(boundary, event, &kind),
        "create_workspace" => rewrite(event, &kind, "path", |raw| boundary.resolve_workspace(raw)),
        "file_list" => explorer_listing(boundary, event, &kind),
        "file_open" | "reveal_path" => explorer_open(boundary, event, &kind),
        "file_save" => explorer_save(boundary, event, &kind),
        "file_create" | "dir_create" => explorer_create(boundary, event, &kind),
        "path_rename" => explorer_rename(boundary, event, &kind),
        "path_move" => explorer_move(boundary, event, &kind),
        "path_trash" => explorer_trash(boundary, event, &kind),
        "terminal_attachment" => attachments(event),
        _ => None,
    }
}

/// The path a client sent, as written; a field the event omits reads as empty
/// and fails the shape check like any other empty path.
fn payload_str(event: &Value, field: &str) -> String {
    event
        .pointer(&format!("/payload/{field}"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

/// The frame a refused path is answered with, and the one log line it leaves.
fn refused(kind: &str, path: &str, refusal: Refusal) -> Value {
    log_path_refusal(kind, path, refusal);
    json!({
        "type": "path_refused",
        "payload": {"kind": kind, "path": path, "reason": refusal.code()},
    })
}

/// Checks one path field and rewrites it to the spelling that was checked.
fn rewrite(
    event: &mut Value,
    kind: &str,
    field: &str,
    resolve: impl Fn(&str) -> Result<PathBuf, Refusal>,
) -> Option<Value> {
    let raw = payload_str(event, field);
    match resolve(&raw) {
        Ok(real) => {
            event["payload"][field] = Value::String(real.display().to_string());
            None
        }
        Err(refusal) => Some(refused(kind, &raw, refusal)),
    }
}

/// A local `remote_file_list` is answered here; a remote one is not this
/// boundary's to judge and is forwarded as it came.
fn registration_listing(boundary: &Boundary, event: &mut Value, kind: &str) -> Option<Value> {
    let local = event
        .pointer("/payload/target_id")
        .and_then(Value::as_str)
        .is_some_and(|target| target == "local");
    if !local {
        return None;
    }
    let raw = payload_str(event, "root_path");
    match boundary.list(&raw) {
        Ok(listing) => Some(listing_frame(kind, listing)),
        Err(refusal) => Some(refused(kind, &raw, refusal)),
    }
}

/// A `file_list` names the checkout root and the folder under it, and is
/// answered here: the Explorer reads folders lazily, so the frame carries one
/// folder's children and the client asks again as the operator expands.
fn explorer_listing(boundary: &Boundary, event: &mut Value, kind: &str) -> Option<Value> {
    let root = payload_str(event, "root");
    let Some(known) = boundary.known_root(&root) else {
        return Some(refused(kind, &root, Refusal::OutsideCheckout));
    };
    let raw = payload_str(event, "path");
    match boundary.list_children(&known, &raw) {
        Ok(listing) => Some(listing_frame(kind, listing)),
        Err(refusal) => Some(refused(kind, &raw, refusal)),
    }
}

/// The frame a listing is answered with. `kind` is the event that asked, so a
/// client routes the answer to the flow that requested it: the registration
/// autocomplete reads the last `remote_file_list` answer and the Explorer keeps
/// one listing per folder it has expanded.
fn listing_frame(kind: &str, listing: Listing) -> Value {
    let mut frame = json!({"type": "directory_list", "payload": listing});
    frame["payload"]["kind"] = Value::String(kind.to_owned());
    frame
}

/// An open or a reveal names the checkout the path belongs to, so the path is
/// checked against that checkout's root. A pair this daemon holds no root for
/// falls back to any root; the core answers the unknown checkout itself.
fn explorer_open(boundary: &Boundary, event: &mut Value, kind: &str) -> Option<Value> {
    let workspace_id = payload_str(event, "workspace_id");
    let checkout_id = payload_str(event, "checkout_id");
    rewrite(event, kind, "path", |raw| {
        boundary.resolve_checkout(&workspace_id, &checkout_id, raw)
    })
}

/// A save carries a path the client already opened, and the core compares it
/// with the path it stored, so it is checked and left as it came.
fn explorer_save(boundary: &Boundary, event: &mut Value, kind: &str) -> Option<Value> {
    let raw = payload_str(event, "path");
    if boundary.is_under_root(&raw) {
        return None;
    }
    Some(refused(kind, &raw, Refusal::OutsideCheckout))
}

/// The root an explorer change names and one path under it: the root has to be
/// one of the registered ones, and both are rewritten to the spelling that was
/// checked.
fn explorer_rooted_path(
    boundary: &Boundary,
    event: &mut Value,
    kind: &str,
    field: &str,
) -> Option<Value> {
    let root = payload_str(event, "root");
    let Some(known) = boundary.known_root(&root) else {
        return Some(refused(kind, &root, Refusal::OutsideCheckout));
    };
    let raw = payload_str(event, field);
    let real = match boundary.resolve_below(&known, &raw) {
        Ok(real) => real,
        Err(refusal) => return Some(refused(kind, &raw, refusal)),
    };
    event["payload"]["root"] = Value::String(known.display().to_string());
    event["payload"][field] = Value::String(real.display().to_string());
    None
}

/// A creation names the folder it lands in and the name it takes.
fn explorer_create(boundary: &Boundary, event: &mut Value, kind: &str) -> Option<Value> {
    let name = payload_str(event, "name");
    if !boundary::valid_name(&name) {
        return Some(refused(kind, &name, Refusal::InvalidPath));
    }
    explorer_rooted_path(boundary, event, kind, "parent")
}

/// A rename names the item and the name it takes.
fn explorer_rename(boundary: &Boundary, event: &mut Value, kind: &str) -> Option<Value> {
    let name = payload_str(event, "name");
    if !boundary::valid_name(&name) {
        return Some(refused(kind, &name, Refusal::InvalidPath));
    }
    explorer_rooted_path(boundary, event, kind, "path")
}

/// A move names the item and the folder it lands in; both are under the root.
fn explorer_move(boundary: &Boundary, event: &mut Value, kind: &str) -> Option<Value> {
    if let Some(frame) = explorer_rooted_path(boundary, event, kind, "path") {
        return Some(frame);
    }
    let root = payload_str(event, "root");
    rewrite(event, kind, "destination", |raw| {
        boundary.resolve_in_root(&root, raw)
    })
}

/// A trash names the item and the row the tree selects once it is gone; both
/// are under the root, and the core still refuses a selection inside the item.
fn explorer_trash(boundary: &Boundary, event: &mut Value, kind: &str) -> Option<Value> {
    if let Some(frame) = explorer_rooted_path(boundary, event, kind, "path") {
        return Some(frame);
    }
    let root = payload_str(event, "root");
    rewrite(event, kind, "select_after", |raw| {
        boundary.resolve_in_root(&root, raw)
    })
}

/// An attachment carries paths the shell staged itself - a screenshot it wrote
/// for the clipboard, a file the operator dragged in - so the boundary checks
/// the shape of each rather than a root. A path that fails the shape, and any
/// path past the batch cap, is dropped here and the drop is logged; the core
/// refuses the whole batch, which would lose the ones that were fine.
fn attachments(event: &mut Value) -> Option<Value> {
    let paths = event.pointer("/payload/paths").and_then(Value::as_array)?;
    let sent = paths.len();
    let kept: Vec<Value> = paths
        .iter()
        .filter(|path| path.as_str().is_some_and(boundary::valid_attachment_path))
        .take(boundary::MAX_ATTACHMENT_FILES)
        .cloned()
        .collect();
    if kept.len() == sent {
        return None;
    }
    eprintln!(
        "{}",
        json!({
            "component": "hided",
            "kind": "attachment.dropped",
            "sent": sent,
            "kept": kept.len(),
        })
    );
    event["payload"]["paths"] = Value::Array(kept);
    None
}

/// Characters of a refused path the log keeps; the path is client input, so
/// the log line is capped rather than grown with it.
const LOGGED_PATH_CAP: usize = 256;

fn log_path_refusal(kind: &str, path: &str, refusal: Refusal) {
    let logged: String = path.chars().take(LOGGED_PATH_CAP).collect();
    eprintln!(
        "{}",
        json!({
            "component": "hided",
            "kind": "path.refused",
            "event": kind,
            "reason": refusal.code(),
            "path": logged,
            "path_truncated": logged.len() < path.len(),
        })
    );
}

/// Which frame a delta read turned into, decided by the cursors alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameKind {
    /// Self-contained: the client replaces everything it holds.
    Snapshot,
    /// Applies on top of what the client already holds.
    Delta,
}

/// Classifies the frame a read from `have_revision`/`have_sequence` produced.
///
/// `None` means the core could not serve the cursor the client sent: it
/// dropped terminal chunks the client never saw, or the client's revision is
/// ahead of the core's (the daemon restarted), which the core answers as a
/// fresh reader. Either way the client state is not one a delta can be
/// applied to, and the caller re-reads from zero and sends a snapshot.
pub fn classify_frame(
    have_revision: u64,
    payload_revision: Option<u64>,
    chunks_dropped: bool,
) -> Option<FrameKind> {
    if have_revision == 0 {
        return Some(FrameKind::Snapshot);
    }
    if chunks_dropped || payload_revision.is_some_and(|revision| revision < have_revision) {
        return None;
    }
    Some(FrameKind::Delta)
}

async fn read_delta(state: &AppState, have_revision: u64, have_sequence: u64) -> Result<Value, ()> {
    let snapshot = state
        .core
        .snapshot(have_revision, have_sequence)
        .map_err(|error| log_snapshot_failure("core", &error))?;
    if snapshot.bytes.is_empty() {
        log_snapshot_failure("empty", "the core returned no bytes");
        return Err(());
    }
    serde_json::from_slice(&snapshot.bytes)
        .map_err(|error| log_snapshot_failure("decode", &error.to_string()))
}

/// A frame the daemon could not produce ends the client's loop, so the
/// client sees a bare close; this record is the only trace of why.
fn log_snapshot_failure(stage: &str, message: &str) {
    eprintln!(
        "{}",
        json!({
            "component": "hided",
            "kind": "ws.snapshot_failed",
            "stage": stage,
            "message": message,
        })
    );
}

async fn send_snapshot(
    socket: &mut WebSocket,
    state: &AppState,
    have_revision: &mut u64,
    have_sequence: &mut u64,
) -> Result<(), ()> {
    let mut payload = read_delta(state, *have_revision, *have_sequence).await?;
    let kind = match classify_frame(
        *have_revision,
        payload.get("revision").and_then(Value::as_u64),
        payload
            .get("chunks_dropped")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    ) {
        Some(kind) => kind,
        None => {
            payload = read_delta(state, 0, 0).await?;
            FrameKind::Snapshot
        }
    };
    if let Some(revision) = payload.get("revision").and_then(Value::as_u64) {
        *have_revision = revision;
    }
    if let Some(sequence) = payload.get("terminal_sequence").and_then(Value::as_u64) {
        *have_sequence = sequence;
    }
    let envelope = json!({
        "type": match kind {
            FrameKind::Snapshot => "snapshot",
            FrameKind::Delta => "delta",
        },
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
        Some("woff") => "font/woff",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_client_gets_a_snapshot_and_a_resumed_one_a_delta() {
        assert_eq!(classify_frame(0, Some(7), false), Some(FrameKind::Snapshot));
        assert_eq!(classify_frame(0, Some(7), true), Some(FrameKind::Snapshot));
        assert_eq!(classify_frame(7, Some(7), false), Some(FrameKind::Delta));
        assert_eq!(classify_frame(5, Some(7), false), Some(FrameKind::Delta));
    }

    #[test]
    fn a_gap_makes_the_server_start_over() {
        assert_eq!(classify_frame(7, Some(7), true), None, "dropped chunks");
        assert_eq!(
            classify_frame(9, Some(7), false),
            None,
            "revision from the future"
        );
    }

    #[test]
    fn token_compare_needs_the_whole_token() {
        assert!(token_matches("abc", "abc"));
        assert!(!token_matches("ab", "abc"));
        assert!(!token_matches("abd", "abc"));
        assert!(!token_matches("", "abc"));
    }
}
