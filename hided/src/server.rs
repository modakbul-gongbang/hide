use std::collections::HashSet;
use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::Notify;

use crate::attachments::{self, Attachments};
use crate::boundary::{self, Boundary, Listing, Refusal};
use crate::core::CoreHandle;
use crate::index::{IndexAnswer, IndexService};
use crate::opener::OpenHandler;
use crate::pane_auth::Registry;
use crate::state_file::{MAX_CLIENTS, SCHEMA_VERSION};
use crate::watch::WatchService;

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
    /// Follows the core's checkout roots into `boundary`; a refused event
    /// catches it up before it is answered.
    pub roots: Arc<crate::RootFollower>,
    /// The daemon's one watch service; every client subscribes to its frames.
    pub watch: Arc<WatchService>,
    /// The ⌘P index cache, one lazy index per registered checkout.
    pub index: Arc<IndexService>,
    /// Staged dropped files on their way to the core's attachment directory.
    pub attachments: Arc<Attachments>,
    /// One bounded, daemon-owned path for OS file associations.
    pub opener: OpenHandler,
    pub token: Arc<String>,
    pub pane_capabilities: Arc<Registry>,
    pub herdr_socket: Option<PathBuf>,
    pub allowed_origins: Arc<HashSet<String>>,
    pub clients: Arc<AtomicUsize>,
    /// Authenticated shell windows, distinct from CLI and non-rendering clients.
    pub renderers: Arc<AtomicUsize>,
    /// Numbers connections so a stage can be released with its connection.
    pub connections: Arc<AtomicU64>,
    pub last_client_gone: Arc<Mutex<Instant>>,
    pub keep_alive: bool,
    pub idle_secs: u64,
    pub shutdown: Arc<Notify>,
    pub ui_dir: Option<PathBuf>,
    pub version: &'static str,
    /// Which connections are looking at the Settings agents tab; the daemon
    /// owns the core's one observation flag on their behalf.
    pub demand: Arc<crate::demand::ObservationDemand>,
    /// What the Settings General tab reads about this daemon, sent once after
    /// a handshake. No token, no environment beyond the paths it names.
    pub daemon_info: Arc<Value>,
}

#[derive(Debug, Deserialize)]
struct Handshake {
    token: String,
    schema_version: u32,
    client_kind: Option<String>,
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
        "open_handlers_in_flight": state.opener.in_flight(),
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
    let connection = state.connections.fetch_add(1, Ordering::SeqCst);
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
    if handshake.schema_version != SCHEMA_VERSION {
        refuse(&mut socket, CloseReason::SchemaMismatch, None).await;
        return;
    }
    if !token_matches(&handshake.token, &state.token) {
        if let Some(capability) = state.pane_capabilities.get(&handshake.token) {
            scoped_client_loop(
                socket,
                state,
                connection,
                handshake.token,
                capability.one_shot,
            )
            .await;
        } else {
            refuse(&mut socket, CloseReason::InvalidToken, None).await;
        }
        return;
    }
    let previous = state.clients.fetch_add(1, Ordering::SeqCst);
    if previous >= MAX_CLIENTS {
        state.clients.fetch_sub(1, Ordering::SeqCst);
        refuse(&mut socket, CloseReason::ClientLimit, Some(previous + 1)).await;
        return;
    }
    let renderer = matches!(handshake.client_kind.as_deref(), Some("web" | "desktop"));
    if renderer {
        state.renderers.fetch_add(1, Ordering::SeqCst);
    }
    // A reconnecting client resumes from the cursors it last applied, so the
    // first frame carries only what changed while it was away; a fresh client
    // (cursor 0) gets the whole state.
    let mut have_revision = handshake.have_revision.unwrap_or(0);
    let mut have_sequence = handshake.have_terminal_sequence.unwrap_or(0);
    let mut notify = state.core.notify.subscribe();
    // A change in a watched folder is announced on this socket beside the
    // snapshot stream; the client re-reads the one folder it names (B2).
    let mut directory_changes = state.watch.subscribe();
    // Answers a device's helper gives for this client alone (the Explorer's
    // listing of a device checkout) arrive here from their own tasks, so a
    // slow device never holds up this socket's snapshot stream.
    let (device_frames_tx, mut device_frames) = tokio::sync::mpsc::channel::<String>(16);
    // Device file reads for this client, each in its own task: their frames
    // come back through `device_bytes`, at most two ranges ahead of the
    // socket, and every read ends with the connection.
    let (device_bytes_tx, mut device_bytes) = tokio::sync::mpsc::channel::<Message>(2);
    let mut device_reads: std::collections::VecDeque<DeviceRead> =
        std::collections::VecDeque::new();
    let daemon = json!({"type": "daemon", "payload": state.daemon_info.as_ref()});
    if socket
        .send(Message::Text(daemon.to_string().into()))
        .await
        .is_err()
    {
        client_gone(&state, connection, renderer);
        return;
    }
    if send_snapshot(&mut socket, &state, &mut have_revision, &mut have_sequence)
        .await
        .is_err()
    {
        client_gone(&state, connection, renderer);
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
            changed = directory_changes.recv() => {
                match changed {
                    Ok(frame) => {
                        let value = serde_json::from_str::<Value>(&frame).ok();
                        let field = |name: &str| {
                            value
                                .as_ref()
                                .and_then(|value| value.pointer(&format!("/payload/{name}")))
                                .and_then(Value::as_str)
                        };
                        // A folder on this machine is re-checked against its
                        // checkout roots; a device's folder names a path there,
                        // which only that device's catalog roots can vouch for.
                        let admitted = match (field("path"), field("device_id")) {
                            (Some(path), Some(herdr_core::workspace::LOCAL_DEVICE_ID) | None) => {
                                state.boundary.resolve_target(path).is_ok()
                            }
                            (Some(path), Some(device)) => state.boundary.is_under_device_root(device, path),
                            (None, _) => false,
                        };
                        if !admitted {
                            continue;
                        }
                        if socket.send(Message::Text(frame.into())).await.is_err() {
                            break;
                        }
                    }
                    // A client that fell behind on folder changes keeps its
                    // snapshot stream; the tree re-reads on the next change.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {}
                }
            }
            Some(frame) = device_bytes.recv() => {
                if socket.send(frame).await.is_err() {
                    break;
                }
            }
            Some(frame) = device_frames.recv() => {
                if socket.send(Message::Text(frame.into())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        match handle_client_text(&state, &text, connection) {
                            Ok(ClientAction::FileBytes(event)) => {
                                if let Some(device) = event_device(&event) {
                                    let superseded = start_device_read(&state, &mut device_reads, device_bytes_tx.clone(), device, event);
                                    // Sent here rather than through `device_bytes`,
                                    // which this loop drains and could be full.
                                    if let Some(frame) = superseded
                                        && socket.send(frame).await.is_err()
                                    {
                                        break;
                                    }
                                } else if send_file_bytes(&mut socket, &state.boundary, &state.roots, &event).await.is_err() {
                                    break;
                                }
                            }
                            Ok(ClientAction::DeviceListing(event)) => {
                                spawn_device_listing(&state, event, device_frames_tx.clone());
                            }
                            outcome => {
                                let replies = match outcome {
                                    Ok(ClientAction::Replies(frames)) => frames,
                                    Err(error) => vec![Message::Text(
                                        json!({"type":"error","payload":{},"message": error}).to_string().into(),
                                    )],
                                    _ => unreachable!(),
                                };
                                let mut failed = false;
                                for frame in replies {
                                    if socket.send(frame).await.is_err() { failed = true; break; }
                                }
                                if failed { break; }
                            }
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        if let Some((request_id, reason)) = state.attachments.receive(connection, &bytes)
                            && socket
                                .send(Message::Text(
                                    attachment_refused(&request_id, reason).to_string().into(),
                                ))
                                .await
                                .is_err()
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
    for read in device_reads {
        read.task.abort();
    }
    client_gone(&state, connection, renderer);
}

/// A pane capability can submit only Workspace commands. It never receives a
/// snapshot, file bytes, or the shell's unrestricted dispatch channel.
async fn scoped_client_loop(
    mut socket: WebSocket,
    state: AppState,
    connection: u64,
    token: String,
    one_shot: bool,
) {
    let previous = state.clients.fetch_add(1, Ordering::SeqCst);
    if previous >= MAX_CLIENTS {
        state.clients.fetch_sub(1, Ordering::SeqCst);
        refuse(&mut socket, CloseReason::ClientLimit, Some(previous + 1)).await;
        return;
    }
    let incoming = tokio::time::timeout(Duration::from_secs(10), socket.recv()).await;
    let response = match incoming {
        Ok(Some(Ok(Message::Text(text)))) if text.len() <= 16 * 1024 => {
            match serde_json::from_str::<Value>(&text) {
                Ok(value) => {
                    let request_id = value["request_id"].as_str().unwrap_or("");
                    if value["type"] != "workspace_query"
                        || request_id.is_empty()
                        || request_id.len() > 64
                        || !request_id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                        || !matches!(value["query"].as_str(), Some("info" | "view_list"))
                    {
                        json!({"type":"workspace_result","ok":false,"reason":"invalid_request","next_action":"Run hide workspace info or hide view list with valid arguments"})
                    } else {
                        let request_id = request_id.to_owned();
                        let query = if value["query"] == "info" {
                            herdr_core::workspace_control::Query::Info
                        } else {
                            herdr_core::workspace_control::Query::ViewList
                        };
                        let core = Arc::clone(&state.core);
                        let registry = Arc::clone(&state.pane_capabilities);
                        let renderers = Arc::clone(&state.renderers);
                        let herdr_socket = state.herdr_socket.clone();
                        let query_token = token.clone();
                        let outcome = tokio::task::spawn_blocking(move || {
                            let socket = herdr_socket
                                .as_deref()
                                .ok_or(("pane_unavailable", "Reconnect Hide to Herdr and retry"))?;
                            let cap = registry
                                .validate(&query_token, socket, &core)
                                .map_err(|reason| (reason, "Reconnect the pane and retry"))?;
                            if renderers.load(Ordering::SeqCst) == 0 {
                                return Err((
                                    "renderer_unavailable",
                                    "Open Hide's web or desktop shell and retry",
                                ));
                            }
                            let result = core
                                .workspace_query(&cap.context.device_id, &cap.pane_id, query)
                                .map_err(|refusal| (refusal.reason, refusal.next_action))?;
                            if result.context != cap.context {
                                return Err(("pane_changed", "Reconnect the pane and retry"));
                            }
                            if renderers.load(Ordering::SeqCst) == 0 {
                                return Err((
                                    "renderer_unavailable",
                                    "Open Hide's web or desktop shell and retry",
                                ));
                            }
                            Ok::<_, (&str, &str)>(result)
                        })
                        .await;
                        match outcome {
                            Ok(Ok(result)) => {
                                json!({"type":"workspace_result","request_id":request_id,"ok":true,"result":result})
                            }
                            Ok(Err((reason, next_action))) => {
                                json!({"type":"workspace_result","request_id":request_id,"ok":false,"reason":reason,"next_action":next_action})
                            }
                            Err(_) => {
                                json!({"type":"workspace_result","request_id":request_id,"ok":false,"reason":"query_unavailable","next_action":"Retry after reconnecting Hide"})
                            }
                        }
                    }
                }
                Err(_) => {
                    json!({"type":"workspace_result","ok":false,"reason":"invalid_request","next_action":"Check the command arguments and retry"})
                }
            }
        }
        _ => {
            json!({"type":"workspace_result","ok":false,"reason":"request_timeout","next_action":"Check Hide status and retry"})
        }
    };
    let _ = socket
        .send(Message::Text(response.to_string().into()))
        .await;
    let _ = socket.send(Message::Close(None)).await;
    if one_shot {
        state.pane_capabilities.revoke(&token);
    }
    client_gone(&state, connection, false);
}

/// How many device file reads one client runs at once. A viewer asks for one
/// file; a read beyond this is most likely for a view the page has left, so
/// the oldest read is ended to make room and answered as `superseded`, so
/// whatever waits on it settles (D-15).
const DEVICE_READS_PER_CLIENT: usize = 2;

struct DeviceRead {
    request_id: String,
    path: String,
    task: tokio::task::AbortHandle,
}

/// Starts a device read and returns the error frame for the read it ended to
/// make room, if any.
fn start_device_read(
    state: &AppState,
    reads: &mut std::collections::VecDeque<DeviceRead>,
    frames: tokio::sync::mpsc::Sender<Message>,
    device: String,
    event: Value,
) -> Option<Message> {
    reads.retain(|read| !read.task.is_finished());
    let mut superseded = None;
    if reads.len() >= DEVICE_READS_PER_CLIENT
        && let Some(oldest) = reads.pop_front()
    {
        oldest.task.abort();
        eprintln!(
            "{}",
            json!({
                "component": "hided", "kind": "device.file_bytes_ended",
                "device": device,
                "request_id": oldest.request_id.chars().take(LOGGED_PATH_CAP).collect::<String>(),
                "reason": "a newer read for this client took its place",
            })
        );
        superseded = Some(file_bytes_error(
            &oldest.request_id,
            &oldest.path,
            "superseded",
        ));
    }
    // Kept to answer this read by its own id if a newer one ends it.
    let (request_id, path) = (
        payload_str(&event, "request_id"),
        payload_str(&event, "path"),
    );
    let task = tokio::spawn(stream_device_file_bytes(
        frames,
        Arc::clone(&state.core),
        Arc::clone(&state.boundary),
        Arc::clone(&state.roots),
        device,
        event,
    ));
    reads.push_back(DeviceRead {
        request_id,
        path,
        task: task.abort_handle(),
    });
    superseded
}

/// Constant in the token's length, so a byte-by-byte mismatch does not leak
/// how much of the token a caller guessed.
fn token_matches(offered: &str, expected: &str) -> bool {
    use subtle::ConstantTimeEq;
    offered.len() == expected.len() && offered.as_bytes().ct_eq(expected.as_bytes()).into()
}

/// Forwards a client event to the core, or answers it here.
///
/// An empty answer means the event went to the core, which answers through
/// the snapshot stream. Anything the daemon answers itself is one or more
/// frames for this client alone: a directory listing, a path refusal, or the
/// binary frames of a `file_bytes` read.
enum ClientAction {
    FileBytes(Value),
    /// A `file_list` for a checkout on an SSH device, answered by its helper.
    DeviceListing(Value),
    Replies(Vec<Message>),
}

/// The file events a checkout on an SSH device answers through that
/// device's helper. This machine's checkout roots and home say nothing about
/// another machine's paths, so these never meet the local boundary: the core
/// finds the checkout and its device in its own catalog, and the helper
/// confines the path to the checkout root it opened, or, for a registration,
/// to that device's own home (`hide_host::register`).
const DEVICE_FILE_EVENTS: [&str; 10] = [
    "create_workspace",
    "file_list",
    "file_open",
    "reveal_path",
    "file_save",
    "file_create",
    "dir_create",
    "path_rename",
    "path_move",
    "path_trash",
];

/// The SSH device a file event names, or `None` for this machine.
fn event_device(event: &Value) -> Option<String> {
    event
        .pointer("/payload/device_id")
        .and_then(Value::as_str)
        .filter(|device| !device.is_empty() && *device != herdr_core::workspace::LOCAL_DEVICE_ID)
        .map(str::to_owned)
}

fn handle_client_text(
    state: &AppState,
    text: &str,
    connection: u64,
) -> Result<ClientAction, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("client json: {error}"))?;
    let mut event = if value.get("schema_version").is_some() && value.get("kind").is_some() {
        value
    } else {
        return Err("expected a core event {schema_version, kind, payload}".to_owned());
    };
    if event.get("kind").and_then(Value::as_str) == Some("file_bytes") {
        return Ok(ClientAction::FileBytes(event));
    }
    if event.get("kind").and_then(Value::as_str) == Some("file_index") {
        return Ok(ClientAction::Replies(handle_file_index(state, &event)));
    }
    match event.get("kind").and_then(Value::as_str) {
        Some("attachment_stage") => {
            return Ok(ClientAction::Replies(handle_attachment_stage(
                state, &event, connection,
            )));
        }
        Some("attachment_commit") => {
            return Ok(ClientAction::Replies(handle_attachment_commit(
                state, &event,
            )));
        }
        Some("attachment_cancel") => {
            state
                .attachments
                .discard(&payload_str(&event, "request_id"));
            return Ok(ClientAction::Replies(Vec::new()));
        }
        Some("ai_settings") => {
            return handle_ai_settings(state, event, connection);
        }
        Some("open_external") => {
            // This token-authenticated socket can be reached through an SSH
            // tunnel. The browser cannot prove it is on the daemon's host,
            // so it has no authority to start the host's OS handler.
            let path = payload_str(&event, "path");
            return Ok(ClientAction::Replies(vec![Message::Text(
                json!({"type":"open_external_result", "payload":{
                    "path":path, "ok":false, "reason":"untrusted_client"
                }})
                .to_string()
                .into(),
            )]));
        }
        _ => {}
    }
    let kind = event.get("kind").and_then(Value::as_str).unwrap_or("");
    if DEVICE_FILE_EVENTS.contains(&kind) && event_device(&event).is_some() {
        if kind == "file_list" {
            return Ok(ClientAction::DeviceListing(event));
        }
        let bytes = serde_json::to_vec(&event).map_err(|error| format!("event encode: {error}"))?;
        return state
            .core
            .dispatch(bytes)
            .map(|()| ClientAction::Replies(Vec::new()));
    }
    if let Some(reply) = admit_event(&state.boundary, &mut event, || roots_current(&state.roots)) {
        return Ok(ClientAction::Replies(vec![Message::Text(
            reply.to_string().into(),
        )]));
    }
    let bytes = serde_json::to_vec(&event).map_err(|error| format!("event encode: {error}"))?;
    state
        .core
        .dispatch(bytes)
        .map(|()| ClientAction::Replies(Vec::new()))
}

/// Lists one folder of a device checkout on a blocking task and hands the
/// answer to the client's socket loop. A client gone by then drops it.
fn spawn_device_listing(state: &AppState, event: Value, frames: tokio::sync::mpsc::Sender<String>) {
    let core = Arc::clone(&state.core);
    let boundary = Arc::clone(&state.boundary);
    let roots = Arc::clone(&state.roots);
    tokio::spawn(async move {
        let frame = tokio::task::spawn_blocking(move || device_listing(&core, &boundary, &roots, &event))
            .await
            .unwrap_or_else(|error| {
                json!({"type": "error", "payload": {}, "message": format!("device listing failed: {error}")})
            });
        let _ = frames.send(frame.to_string()).await;
    });
}

/// One folder of a checkout on an SSH device. The root has to be a checkout
/// the core's catalog carries for that device; the folder is spelled under it
/// and the helper refuses anything that leaves it.
fn device_listing(
    core: &CoreHandle,
    boundary: &Boundary,
    roots: &crate::RootFollower,
    event: &Value,
) -> Value {
    let device = payload_str(event, "device_id");
    let root = payload_str(event, "root");
    let raw = payload_str(event, "path");
    if !device_root_known(boundary, roots, &device, &root) {
        return refused("file_list", &root, Refusal::OutsideCheckout);
    }
    let folder = if raw.is_empty() { root.clone() } else { raw };
    let relative = if folder == root {
        String::new()
    } else {
        match folder
            .strip_prefix(root.trim_end_matches('/'))
            .and_then(|rest| rest.strip_prefix('/'))
            .filter(|rest| hide_host::relative_path(rest).is_ok())
        {
            Some(rest) => rest.to_owned(),
            None => return refused("file_list", &folder, Refusal::OutsideCheckout),
        }
    };
    let unavailable = |code: &str, message: String| {
        eprintln!(
            "{}",
            json!({
                "component": "hided",
                "kind": "device.listing_unavailable",
                "device": device,
                "code": code,
                "message": message,
            })
        );
        json!({"type": "directory_unavailable", "payload": {
            "kind": "file_list", "device_id": device, "root_path": folder, "code": code, "message": message,
        }})
    };
    let channel = match core.device_channel(&device) {
        Ok(channel) => channel,
        Err(message) => return unavailable("not_ready", message),
    };
    use herdr_core::host_access::HostCallError;
    match herdr_core::host_access::list_folder(channel.as_ref(), &root, &relative) {
        Ok(listing) => {
            let base = folder.trim_end_matches('/');
            let entries: Vec<Value> = listing
                .entries
                .into_iter()
                .map(|entry| {
                    json!({
                        "path": format!("{base}/{}", entry.name),
                        "name": entry.name,
                        "is_directory": entry.is_directory,
                        "inode": entry.inode,
                    })
                })
                .collect();
            json!({"type": "directory_list", "payload": {
                "kind": "file_list", "device_id": device, "root_path": folder,
                "entries": entries, "truncated": listing.truncated,
            }})
        }
        Err(HostCallError::NotConnected(message)) => unavailable("not_ready", message),
        Err(error @ HostCallError::Busy) => unavailable("busy", error.to_string()),
        Err(HostCallError::Unknown(message)) => unavailable("unknown", message),
        Err(HostCallError::Refused(error)) => unavailable("refused", error.message),
    }
}

/// Splits an `ai_settings` event: the observation hint is this connection's
/// demand and reaches the core only when the aggregate changes; a provider or
/// model choice goes to the core as it came.
fn handle_ai_settings(
    state: &AppState,
    mut event: Value,
    connection: u64,
) -> Result<ClientAction, String> {
    let observing = event
        .get_mut("payload")
        .and_then(Value::as_object_mut)
        .and_then(|payload| payload.remove("observing"));
    match observing {
        Some(Value::Bool(observing)) => {
            state.demand.set(connection, observing, |aggregate| {
                dispatch_observation(state, connection, aggregate)
            });
        }
        Some(Value::Null) | None => {}
        Some(_) => return Err("ai_settings.observing must be a boolean".to_owned()),
    }
    let carries_choice = event
        .get("payload")
        .and_then(Value::as_object)
        .is_some_and(|payload| !payload.is_empty());
    if carries_choice {
        let bytes = serde_json::to_vec(&event).map_err(|error| format!("event encode: {error}"))?;
        state.core.dispatch(bytes)?;
    }
    Ok(ClientAction::Replies(Vec::new()))
}

/// Runs under the demand lock (`ObservationDemand::set`), so it only logs
/// and hands the event to the core's channel.
fn dispatch_observation(state: &AppState, connection: u64, observing: bool) {
    eprintln!(
        "{}",
        json!({
            "component": "hided",
            "kind": "settings.observation",
            "connection": connection,
            "observing": observing,
        })
    );
    let event = json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "ai_settings",
        "payload": {"observing": observing},
    });
    if let Err(error) = state.core.dispatch(event.to_string().into_bytes()) {
        eprintln!(
            "{}",
            json!({
                "component": "hided",
                "kind": "settings.observation_failed",
                "connection": connection,
                "message": error,
            })
        );
    }
}

/// Opens a checkout file with the host OS handler, or with the program
/// `HIDE_OPEN_COMMAND` names (PRD S3 D-12). The path passes the same
/// checkout-root boundary as every other Explorer path, so the handler can
/// only ever be pointed at a regular file inside a registered root.
#[allow(dead_code)] // Retained for a future transport that can prove local ownership.
fn handle_open_external(state: &AppState, event: &Value) -> Vec<Message> {
    let path = payload_str(event, "path");
    let real = match state.boundary.resolve_file(&path) {
        Ok((real, _)) => real,
        Err(refusal) => {
            return vec![Message::Text(
                refused("open_external", &path, refusal).to_string().into(),
            )];
        }
    };
    // The shell reveals an executable, an application bundle or an installer
    // rather than opening it, and this frame is a page's request rather than
    // the operator's own click, so the same rule holds here (D-12).
    if let Err(reason) = openable(&real) {
        eprintln!(
            "{}",
            json!({
                "component": "hided",
                "kind": "open.external.refused",
                "reason": reason,
            })
        );
        return vec![Message::Text(
            json!({
                "type": "open_external_result",
                "payload": {"path": real.display().to_string(), "ok": false, "reason": reason},
            })
            .to_string()
            .into(),
        )];
    }
    let (ok, reason) = match state.opener.launch(&real) {
        Ok(()) => (true, Value::Null),
        Err(reason) => (false, Value::String(reason.to_owned())),
    };
    eprintln!(
        "{}",
        json!({
            "component": "hided",
            "kind": "open.external",
            "ok": ok,
            "open_handlers_in_flight": state.opener.in_flight(),
        })
    );
    vec![Message::Text(
        json!({
            "type": "open_external_result",
            "payload": {"path": real.display().to_string(), "ok": ok, "reason": reason},
        })
        .to_string()
        .into(),
    )]
}

/// The extensions whose registered handler runs, installs or executes what it
/// opens rather than showing it: a launcher, a terminal script, an installer,
/// a package, a script interpreter's file. A page that can write inside a
/// checkout could otherwise name one and have the operator's own machine start
/// it, which is the line the shell draws for executable paths.
const EXECUTING_EXTENSIONS: &[&str] = &[
    // macOS bundles, packages, profiles and terminal scripts
    "app",
    "pkg",
    "mpkg",
    "dmg",
    "mobileconfig",
    "terminal",
    "term",
    "command",
    "tool",
    "workflow",
    "scpt",
    "scptd",
    // locators: the handler hands the target to something else, Terminal for
    // an ssh:// URL among them
    "webloc",
    "url",
    "inetloc",
    "fileloc",
    // shell and interpreter scripts whose handler runs them on open
    "sh",
    "bash",
    "zsh",
    "csh",
    "fish",
    "ksh",
    "py",
    "pyw",
    "pl",
    "rb",
    "php",
    "lua",
    "jar",
    "class",
    "appimage",
    "run",
    "desktop",
    "service",
    // Windows executables, script hosts and package installers
    "exe",
    "com",
    "scr",
    "pif",
    "bat",
    "cmd",
    "msi",
    "msp",
    "lnk",
    "ps1",
    "psm1",
    "psd1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "hta",
    "jnlp",
    "msc",
    "application",
    "appref-ms",
    "appx",
    "msix",
    "appinstaller",
    // shared libraries
    "dylib",
    "so",
];

/// Whether the host handler may be pointed at this file. An application
/// bundle, an installer, a script a handler would run, anything with an
/// execute bit, or a file whose own header says it is an executable is
/// refused, because a page must not be able to start a program by naming a
/// checkout file (D-12).
fn openable(path: &Path) -> Result<(), &'static str> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if EXECUTING_EXTENSIONS.contains(&extension.as_str()) {
        return Err("not_openable");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(path).map_err(|_| "not_found")?;
        if metadata.permissions().mode() & 0o111 != 0 {
            return Err("not_openable");
        }
    }
    let mut file = fs::File::open(path).map_err(|_| "not_found")?;
    if is_executable_header(&mut file)? {
        return Err("not_openable");
    }
    Ok(())
}

/// Whether the file's own header says it is a program: Mach-O (thin or fat,
/// either byte order), ELF, or the DOS/PE MZ family. A real MZ executable
/// either carries the zero fields its header format requires or the loader
/// signature its header points at, while a document that merely begins with
/// those letters has neither.
fn is_executable_header(file: &mut fs::File) -> Result<bool, &'static str> {
    use std::io::{Read, Seek, SeekFrom};
    let mut head = [0u8; 512];
    let read = file.read(&mut head).map_err(|_| "not_found")?;
    let head = &head[..read];
    if head.len() < 4 {
        return Ok(false);
    }
    let magic4 = [head[0], head[1], head[2], head[3]];
    if matches!(
        magic4,
        // Mach-O 32/64 and their byte-swapped forms, 32 and 64-bit fat
        [0xFE, 0xED, 0xFA, 0xCE]
            | [0xFE, 0xED, 0xFA, 0xCF]
            | [0xCE, 0xFA, 0xED, 0xFE]
            | [0xCF, 0xFA, 0xED, 0xFE]
            | [0xCA, 0xFE, 0xBA, 0xBE]
            | [0xBE, 0xBA, 0xFE, 0xCA]
            | [0xCA, 0xFE, 0xBA, 0xBF]
            | [0xBF, 0xBA, 0xFE, 0xCA]
            // ELF
            | [0x7F, b'E', b'L', b'F']
    ) {
        return Ok(true);
    }
    if head[0] != b'M' || head[1] != b'Z' {
        return Ok(false);
    }
    // Every real MZ family executable has zero fields in its header; a text
    // document that begins with those two letters has none.
    if head.contains(&0) {
        return Ok(true);
    }
    // `e_lfanew` says where the loader signature is. A header that points at
    // PE/NE/LE/LX is a program whatever its extension claims.
    if head.len() < 0x40 {
        return Ok(false);
    }
    let at = u64::from(u32::from_le_bytes([
        head[0x3C], head[0x3D], head[0x3E], head[0x3F],
    ]));
    if at < 0x40 || file.seek(SeekFrom::Start(at)).is_err() {
        return Ok(false);
    }
    let mut signature = [0u8; 4];
    if file.read_exact(&mut signature).is_err() {
        return Ok(false);
    }
    Ok(matches!(
        &signature,
        b"PE\0\0" | b"NE\0\0" | b"LE\0\0" | b"LX\0\0" | b"W4\0\0" | b"DL\0\0"
    ))
}

/// One line of a refused attachment: the request and why nothing was staged.
fn attachment_refused(request_id: &str, reason: &str) -> Value {
    json!({"type": "attachment_refused", "payload": {"request_id": request_id, "reason": reason}})
}

/// Opens one staged upload the client will send bytes for. The caps are
/// checked here so a too-large file is refused before its bytes arrive.
fn handle_attachment_stage(state: &AppState, event: &Value, connection: u64) -> Vec<Message> {
    let request_id = payload_str(event, "request_id");
    let name = payload_str(event, "name");
    let size = event
        .pointer("/payload/size")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let clipboard = event
        .pointer("/payload/clipboard")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match state
        .attachments
        .begin(connection, &request_id, &name, size, clipboard)
    {
        Ok(()) => Vec::new(),
        Err(reason) => vec![Message::Text(
            attachment_refused(&request_id, reason).to_string().into(),
        )],
    }
}

/// Turns staged uploads into the one `terminal_attachment` event the Swift
/// shell sends for a batch, and reports clipboard readiness, because hided
/// staged the file the core is waiting for.
fn handle_attachment_commit(state: &AppState, event: &Value) -> Vec<Message> {
    let request_id = payload_str(event, "request_id");
    let pane_id = payload_str(event, "pane_id");
    if pane_id.is_empty() {
        return vec![Message::Text(
            attachment_refused(&request_id, "no_pane")
                .to_string()
                .into(),
        )];
    }
    let bracketed = event
        .pointer("/payload/bracketed_paste")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let clipboard = event
        .pointer("/payload/clipboard")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let stages: Vec<String> = event
        .pointer("/payload/stages")
        .and_then(Value::as_array)
        .map(|stages| {
            stages
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    // A clipboard paste names no paths: the core reads the image at the path
    // its own request id derives, so the commit must be the stage that wrote
    // it and nothing else.
    if clipboard && (stages.len() != 1 || stages.first().is_none_or(|stage| *stage != request_id)) {
        return vec![Message::Text(
            attachment_refused(&request_id, "invalid_request_id")
                .to_string()
                .into(),
        )];
    }
    if !attachments::valid_request_id(&request_id) {
        return vec![Message::Text(
            attachment_refused(&request_id, "invalid_request_id")
                .to_string()
                .into(),
        )];
    }
    let paths = match state.attachments.commit(&stages, clipboard) {
        Ok(paths) => paths,
        Err(reason) => {
            return vec![Message::Text(
                attachment_refused(&request_id, reason).to_string().into(),
            )];
        }
    };
    let attachment = json!({
        "schema_version": 2,
        "kind": "terminal_attachment",
        "payload": {
            "request_id": request_id,
            "pane_id": pane_id,
            "bracketed_paste": bracketed,
            "clipboard": clipboard,
            "paths": paths,
        },
    });
    if let Err(error) = state
        .core
        .dispatch(serde_json::to_vec(&attachment).unwrap_or_default())
    {
        log_snapshot_failure("attachment", &error);
        return vec![Message::Text(
            attachment_refused(&request_id, "forward_failed")
                .to_string()
                .into(),
        )];
    }
    if clipboard {
        let ready = json!({
            "schema_version": 2,
            "kind": "terminal_attachment_ready",
            "payload": {"request_id": request_id, "pane_id": pane_id, "error": Value::Null},
        });
        if let Err(error) = state
            .core
            .dispatch(serde_json::to_vec(&ready).unwrap_or_default())
        {
            log_snapshot_failure("attachment", &error);
        }
    }
    Vec::new()
}

/// A `file_index` query: the root is checked against the registered checkouts
/// and the daemon answers from its per-root index. The first query for a root
/// starts the walk and answers `indexing: true`; the next one has the list.
fn handle_file_index(state: &AppState, event: &Value) -> Vec<Message> {
    let root = payload_str(event, "root");
    let query = payload_str(event, "query");
    if let Some(device) = event_device(event) {
        return vec![Message::Text(
            device_file_index(state, &device, &root, &query)
                .to_string()
                .into(),
        )];
    }
    let found = match state.boundary.open_directory(Path::new(&root), &root) {
        Err(_) if roots_current(&state.roots) => {
            state.boundary.open_directory(Path::new(&root), &root)
        }
        found => found,
    };
    let Ok((known, opened)) = found else {
        return vec![Message::Text(
            refused("file_index", &root, Refusal::OutsideCheckout)
                .to_string()
                .into(),
        )];
    };
    let root_path = known.display().to_string();
    let walk_root = known.clone();
    let answer = state.index.query(
        herdr_core::workspace::LOCAL_DEVICE_ID,
        &root_path,
        &query,
        move || {
            Ok(hide_host::index::walk(
                &cap_std::fs::Dir::from_std_file(opened),
                &walk_root,
            ))
        },
    );
    vec![Message::Text(
        index_result(
            herdr_core::workspace::LOCAL_DEVICE_ID,
            &root_path,
            &query,
            answer,
            |relative| {
                let path = known.join(relative).display().to_string();
                state.boundary.resolve_target(&path).ok().map(|_| path)
            },
        )
        .to_string()
        .into(),
    )]
}

/// The `file_index_result` frame for one answer. `path_of` gives an entry's
/// absolute path on its device, or `None` to leave an entry out.
fn index_result(
    device: &str,
    root: &str,
    query: &str,
    answer: IndexAnswer,
    path_of: impl Fn(&str) -> Option<String>,
) -> Value {
    let (files, truncated, indexing, unavailable) = match answer {
        IndexAnswer::Indexing => (Vec::new(), false, true, None),
        IndexAnswer::Ready { entries, truncated } => (
            entries
                .iter()
                .filter_map(|relative| {
                    path_of(relative).map(|path| json!({"path": path, "relative_path": relative}))
                })
                .collect(),
            truncated,
            false,
            None,
        ),
        IndexAnswer::Failed(message) => (Vec::new(), false, false, Some(message)),
    };
    json!({"type": "file_index_result", "payload": {
        "device_id": device,
        "root_path": root,
        "query": query,
        "files": files,
        "truncated": truncated,
        "indexing": indexing,
        "unavailable": unavailable,
    }})
}

/// A `file_index` query for a checkout on an SSH device. The root has to be
/// one the core's catalog carries for that device; the walk is the device's
/// helper's, reached on the index worker, and its paths are that device's.
fn device_file_index(state: &AppState, device: &str, root: &str, query: &str) -> Value {
    if !device_root_known(&state.boundary, &state.roots, device, root) {
        return refused("file_index", root, Refusal::OutsideCheckout);
    }
    let core = Arc::clone(&state.core);
    let (walk_device, walk_root) = (device.to_owned(), root.to_owned());
    let answer = state.index.query(device, root, query, move || {
        let channel = core.device_channel(&walk_device)?;
        herdr_core::host_access::index_root(channel.as_ref(), &walk_root).map_err(|error| {
            eprintln!(
                "{}",
                json!({
                    "component": "hided",
                    "kind": "device.index_unavailable",
                    "device": walk_device,
                    "message": error.to_string(),
                })
            );
            error.to_string()
        })
    });
    let base = root.trim_end_matches('/');
    index_result(device, root, query, answer, |relative| {
        Some(format!("{base}/{relative}"))
    })
}

/// Bytes one binary frame carries; a read streams in frames this size so a
/// large file never becomes one unbounded message on the shared socket.
const BYTES_CHUNK: u64 = 4 * 1024 * 1024;

/// The header of a binary frame: a 4-byte big-endian length, the header JSON,
/// then the bytes. The header carries the request it answers, the offset the
/// bytes start at, the file's total size, and whether this frame ends the
/// read, so a client can assemble a range without a second round trip.
fn bytes_frame(header: &Value, bytes: &[u8]) -> Vec<u8> {
    let json = header.to_string();
    let mut out = Vec::with_capacity(4 + json.len() + bytes.len());
    out.extend_from_slice(&(json.len() as u32).to_be_bytes());
    out.extend_from_slice(json.as_bytes());
    out.extend_from_slice(bytes);
    out
}

/// A read the daemon could not serve as bytes: the file is past the read cap,
/// or the disk refused it. The client shows one line and offers no retry.
fn file_bytes_error(request_id: &str, path: &str, reason: &str) -> Message {
    eprintln!(
        "{}",
        json!({
            "component": "hided",
            "kind": "file_bytes.failed",
            "reason": reason,
            "path": path.chars().take(LOGGED_PATH_CAP).collect::<String>(),
        })
    );
    Message::Text(
        json!({
            "type": "file_bytes_error",
            "payload": {"request_id": request_id, "path": path, "reason": reason},
        })
        .to_string()
        .into(),
    )
}

/// A `file_bytes` read of a file on an SSH device. The root has to be a
/// checkout the core's catalog carries for that device and the path is spelled
/// under it; the device's helper confines the read and answers one bounded
/// range per call, so this keeps one range in memory as the local read does.
/// A file that changed between ranges ends the read as `read_failed` rather
/// than joining two files' bytes.
///
/// Each range is a helper round trip that can take seconds, so the read runs
/// in its own task and hands its frames to the client loop through a bounded
/// channel: the socket keeps carrying typing and snapshots meanwhile, and a
/// full channel holds the read back instead of buffering it.
async fn stream_device_file_bytes(
    frames: tokio::sync::mpsc::Sender<Message>,
    core: Arc<CoreHandle>,
    boundary: Arc<Boundary>,
    roots: Arc<crate::RootFollower>,
    device: String,
    event: Value,
) -> Result<(), ()> {
    let (device, event) = (device.as_str(), &event);
    let request_id = payload_str(event, "request_id");
    let path = payload_str(event, "path");
    let root = payload_str(event, "root");
    let offset = event
        .pointer("/payload/offset")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let length = event.pointer("/payload/length").and_then(Value::as_u64);
    let relative = path
        .strip_prefix(root.trim_end_matches('/'))
        .and_then(|rest| rest.strip_prefix('/'))
        .filter(|rest| hide_host::relative_path(rest).is_ok())
        .map(str::to_owned);
    let Some(relative) = relative.filter(|_| device_root_known(&boundary, &roots, device, &root))
    else {
        return frames
            .send(Message::Text(
                refused("file_bytes", &path, Refusal::OutsideCheckout)
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|_| ());
    };
    if length.is_some_and(|length| length > boundary::MAX_FILE_BYTES) {
        return frames
            .send(file_bytes_error(&request_id, &path, "too_large"))
            .await
            .map_err(|_| ());
    }
    let mut cursor = offset;
    let mut end: Option<u64> = None;
    let mut first: Option<hide_host::bytes::FileStamp> = None;
    let mut total: Option<u64> = None;
    loop {
        let wanted = end.map_or(hide_host::bytes::MAX_RANGE, |end| {
            end.saturating_sub(cursor).min(hide_host::bytes::MAX_RANGE)
        });
        let core = Arc::clone(&core);
        let (read_device, read_root, read_relative) =
            (device.to_owned(), root.clone(), relative.clone());
        let range = tokio::task::spawn_blocking(move || {
            let channel = core.device_channel(&read_device)?;
            herdr_core::host_access::read_bytes(
                channel.as_ref(),
                &read_root,
                &read_relative,
                cursor,
                wanted,
            )
            .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result);
        let range = match range {
            Ok(range) => range,
            Err(message) => {
                eprintln!(
                    "{}",
                    json!({
                        "component": "hided",
                        "kind": "device.file_bytes_failed",
                        "device": device,
                        "message": message,
                    })
                );
                return frames
                    .send(file_bytes_error(&request_id, &path, "read_failed"))
                    .await
                    .map_err(|_| ());
            }
        };
        let end_now = *end.get_or_insert_with(|| {
            let wanted = length.unwrap_or_else(|| range.total.saturating_sub(range.offset));
            range.offset.saturating_add(wanted).min(range.total)
        });
        if end_now - range.offset.min(end_now) > boundary::MAX_FILE_BYTES {
            return frames
                .send(file_bytes_error(&request_id, &path, "too_large"))
                .await
                .map_err(|_| ());
        }
        if first.get_or_insert_with(|| range.file.clone()) != &range.file {
            return frames
                .send(file_bytes_error(&request_id, &path, "read_failed"))
                .await
                .map_err(|_| ());
        }
        let expected_total = *total.get_or_insert(range.total);
        let Ok(bytes) = range.bytes() else {
            return frames
                .send(file_bytes_error(&request_id, &path, "read_failed"))
                .await
                .map_err(|_| ());
        };
        let asked = AskedRange {
            offset: cursor,
            length: wanted,
            end: end_now,
            total: expected_total,
        };
        let Some((taken, eof)) = asked.accept(range.offset, range.total, bytes.len()) else {
            return frames
                .send(file_bytes_error(&request_id, &path, "read_failed"))
                .await
                .map_err(|_| ());
        };
        let header = json!({
            "type": "file_bytes",
            "request_id": request_id,
            "path": path,
            "offset": range.offset,
            "total": range.total,
            "eof": eof,
        });
        frames
            .send(Message::Binary(
                bytes_frame(&header, &bytes[..taken]).into(),
            ))
            .await
            .map_err(|_| ())?;
        cursor = range.offset + taken as u64;
        if eof {
            return Ok(());
        }
    }
}

/// One range a device read asked its helper for.
struct AskedRange {
    offset: u64,
    length: u64,
    /// Where the whole read ends.
    end: u64,
    /// The file size the read started with.
    total: u64,
}

impl AskedRange {
    /// The helper's answer is untrusted input: it has to be the range asked
    /// for, of the same file size, no longer than asked, and short only where
    /// the read ends, or a misbehaving helper could stretch one read into
    /// millions of round trips or end it early as if the file were shorter.
    /// Returns how many of the answered bytes to send and whether they end
    /// the read; `None` fails the read.
    fn accept(&self, offset: u64, total: u64, length: usize) -> Option<(usize, bool)> {
        if offset != self.offset || total != self.total || length as u64 > self.length {
            return None;
        }
        let taken = length.min(self.end.saturating_sub(offset) as usize);
        let eof = offset + taken as u64 >= self.end;
        if !eof && (length as u64) < self.length {
            return None;
        }
        Some((taken, eof))
    }
}

/// A `file_bytes` read keeps only one bounded chunk in memory. Sending each
/// frame before reading the next gives the socket backpressure even when all
/// eight authenticated clients ask for the maximum range together.
async fn send_file_bytes(
    socket: &mut WebSocket,
    boundary: &Boundary,
    roots: &crate::RootFollower,
    event: &Value,
) -> Result<(), ()> {
    let request_id = payload_str(event, "request_id");
    let path = payload_str(event, "path");
    let offset = event
        .pointer("/payload/offset")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let length = event.pointer("/payload/length").and_then(Value::as_u64);
    let opened = match boundary.open_file(&path) {
        Err(Refusal::OutsideCheckout) if roots_current(roots) => boundary.open_file(&path),
        opened => opened,
    };
    let (real, file, total) = match opened {
        Ok(source) => source,
        Err(refusal) => {
            return socket
                .send(Message::Text(
                    refused("file_bytes", &path, refusal).to_string().into(),
                ))
                .await
                .map_err(|_| ());
        }
    };
    let wanted = length.unwrap_or_else(|| total.saturating_sub(offset.min(total)));
    if wanted > boundary::MAX_FILE_BYTES {
        return socket
            .send(file_bytes_error(&request_id, &path, "too_large"))
            .await
            .map_err(|_| ());
    }
    let mut file = tokio::fs::File::from_std(file);
    let start = offset.min(total);
    if start > 0 && file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return socket
            .send(file_bytes_error(&request_id, &path, "read_failed"))
            .await
            .map_err(|_| ());
    }
    let end = start.saturating_add(wanted).min(total);
    let displayed = real.display().to_string();
    let mut cursor = start;
    loop {
        let take = (end - cursor).min(BYTES_CHUNK) as usize;
        let mut buffer = vec![0u8; take];
        let read = match file.read(&mut buffer).await {
            Ok(read) => read,
            Err(_) => {
                return socket
                    .send(file_bytes_error(&request_id, &path, "read_failed"))
                    .await
                    .map_err(|_| ());
            }
        };
        buffer.truncate(read);
        // A file that shrank between the size read and this read answers 0
        // bytes; that ends the stream rather than spinning on it.
        let eof = read == 0 || cursor + read as u64 >= end;
        let header = json!({
            "type": "file_bytes",
            "request_id": request_id,
            "path": displayed,
            "offset": cursor,
            "total": total,
            "eof": eof,
        });
        socket
            .send(Message::Binary(bytes_frame(&header, &buffer).into()))
            .await
            .map_err(|_| ())?;
        cursor += read as u64;
        if eof {
            return Ok(());
        }
    }
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
/// The shell's own attachment events are hided's to send (a web client stages
/// bytes through `attachment_*` instead), so a client that sends one is
/// answered with an error frame and it never reaches the core: those events
/// name arbitrary paths for the core to read. Every other kind passes
/// untouched to the core.
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
        "browser_open" | "browser_state" => browser_url(boundary, event, &kind),
        "view_layout"
            if event.pointer("/payload/action").and_then(Value::as_str) == Some("navigate") =>
        {
            browser_url(boundary, event, &kind)
        }
        // The shell's attachment events name files the operator's machine
        // shows it; hided is their only producer for a web client (which
        // stages bytes instead), so a client that sends one is naming an
        // arbitrary path and never reaches the core.
        "terminal_attachment" | "terminal_attachment_ready" | "terminal_attachment_action" => {
            rejected(kind.as_str())
        }
        _ => None,
    }
}

/// Runs the boundary on a client event. The roots follow the core on their own
/// reads, so an event can name a checkout this client read in a snapshot the
/// boundary has not applied yet: after a launch, a checkout the core learns
/// from Herdr's first sync reaches both on separate reads. A refusal as
/// outside every checkout therefore brings the roots current (`catch_up`) and
/// runs the check once more before it is answered. The first check's
/// `path.refused` line stays in the log, followed by `boundary.roots_caught_up`
/// when the second one let the event through.
fn admit_event(
    boundary: &Boundary,
    event: &mut Value,
    catch_up: impl FnOnce() -> bool,
) -> Option<Value> {
    let reply = apply_boundary(boundary, event);
    if !reply.as_ref().is_some_and(refused_outside_checkout) || !catch_up() {
        return reply;
    }
    let again = apply_boundary(boundary, event);
    if !again.as_ref().is_some_and(refused_outside_checkout) {
        eprintln!(
            "{}",
            json!({
                "component": "hided",
                "kind": "boundary.roots_caught_up",
                "event": event.get("kind").and_then(Value::as_str).unwrap_or(""),
            })
        );
    }
    again
}

/// Whether a boundary answer refused a path as outside every checkout.
fn refused_outside_checkout(frame: &Value) -> bool {
    frame.get("type").and_then(Value::as_str) == Some("path_refused")
        && frame.pointer("/payload/reason").and_then(Value::as_str)
            == Some(Refusal::OutsideCheckout.code())
}

/// Brings the roots current with the core for a check that found no root:
/// true once they are, whether this read or one it waited for applied them,
/// so the check is worth running again. False only when the core is gone.
fn roots_current(roots: &crate::RootFollower) -> bool {
    match roots.catch_up() {
        Ok(_) => true,
        Err(error) => {
            crate::roots_failed(&error);
            false
        }
    }
}

/// Whether `root` is a checkout the core's catalog carries for `device`,
/// bringing the roots current once before saying no (see `admit_event`).
fn device_root_known(
    boundary: &Boundary,
    roots: &crate::RootFollower,
    device: &str,
    root: &str,
) -> bool {
    boundary.is_device_root(device, root)
        || (roots_current(roots) && boundary.is_device_root(device, root))
}

/// The path a client sent, as written; a field the event omits reads as empty
/// and is refused like any other empty path.
fn payload_str(event: &Value, field: &str) -> String {
    event
        .pointer(&format!("/payload/{field}"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

/// The frame a client-sent shell-only event is answered with: the event is
/// never forwarded, so a path it carries cannot reach the core.
fn rejected(kind: &str) -> Option<Value> {
    eprintln!(
        "{}",
        json!({
            "component": "hided",
            "kind": "client.rejected",
            "event": kind,
            "reason": "daemon_only_event",
        })
    );
    Some(json!({
        "type": "error",
        "payload": {},
        "message": format!("{kind} is sent by the daemon, not by a client"),
    }))
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

/// A browser display's address. A `file:` URL names a local file, so its
/// path has to be under a registered checkout like any path a client sends,
/// and it is written back as the URL of the path that was checked. Any other
/// address is the core's to judge.
fn browser_url(boundary: &Boundary, event: &mut Value, kind: &str) -> Option<Value> {
    let raw = payload_str(event, "url");
    if !crate::file_url::is_file_url(&raw) {
        return None;
    }
    let Some((path, suffix)) = crate::file_url::file_path(&raw) else {
        return Some(refused(kind, &raw, Refusal::InvalidPath));
    };
    match boundary.resolve_target(&path) {
        Ok(real) => {
            let url = crate::file_url::file_url(&real.display().to_string(), suffix);
            event["payload"]["url"] = Value::String(url);
            None
        }
        Err(refusal) => Some(refused(kind, &path, refusal)),
    }
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

fn client_gone(state: &AppState, connection: u64, renderer: bool) {
    if renderer {
        state.renderers.fetch_sub(1, Ordering::SeqCst);
    }
    state.attachments.release(connection);
    state.demand.release(connection, |observing| {
        dispatch_observation(state, connection, observing)
    });
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
        // Vite emits a lazy-loaded ES module (the pdf.js worker among them) as
        // `.mjs`; a module script is refused unless its type is JavaScript.
        Some("mjs") => "text/javascript; charset=utf-8",
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

    /// A device range is accepted only as the range asked for: same offset
    /// and file size, never longer, and short only where the read ends
    /// (PRD S5.5 B39, B43).
    #[test]
    fn a_device_range_answer_is_the_range_asked_for_or_the_read_fails() {
        const MIB: u64 = 1024 * 1024;
        let asked = AskedRange {
            offset: 0,
            length: 4 * MIB,
            end: 10 * MIB,
            total: 10 * MIB,
        };
        let full = (4 * MIB) as usize;
        assert_eq!(asked.accept(0, 10 * MIB, full), Some((full, false)));
        // One byte per range would take ten million round trips.
        assert_eq!(asked.accept(0, 10 * MIB, 1), None);
        // An empty answer before the end is not the end of the file.
        assert_eq!(asked.accept(0, 10 * MIB, 0), None);
        assert_eq!(asked.accept(0, 10 * MIB, full + 1), None);
        assert_eq!(asked.accept(1, 10 * MIB, full), None);
        assert_eq!(asked.accept(0, 11 * MIB, full), None);

        // The last range is short because the read ends there.
        let last = AskedRange {
            offset: 8 * MIB,
            length: 2 * MIB,
            end: 10 * MIB,
            total: 10 * MIB,
        };
        assert_eq!(
            last.accept(8 * MIB, 10 * MIB, (2 * MIB) as usize),
            Some(((2 * MIB) as usize, true))
        );
        // A read of an empty file ends at once.
        let empty = AskedRange {
            offset: 0,
            length: 4 * MIB,
            end: 0,
            total: 0,
        };
        assert_eq!(empty.accept(0, 0, 0), Some((0, true)));
        // A read that asked for less than the file ends at its own end.
        let part = AskedRange {
            offset: 0,
            length: 4 * MIB,
            end: MIB,
            total: 10 * MIB,
        };
        assert_eq!(part.accept(0, 10 * MIB, full), Some((MIB as usize, true)));
    }

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
    fn the_host_handler_never_gets_a_program() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("notes.txt");
        std::fs::write(&plain, "x").unwrap();
        assert_eq!(openable(&plain), Ok(()), "an ordinary file opens");
        assert_eq!(
            openable(&dir.path().join("huge.md")),
            Err("not_found"),
            "a file that vanished between the boundary check and the handler is refused"
        );

        // A handler that runs what it opens, on any platform, is refused by
        // name; a plain text file with the same body is not.
        for name in [
            "run.sh",
            "run.tool",
            "app.app",
            "installer.pkg",
            "image.dmg",
            "term.terminal",
            "session.term",
            "job.command",
            "link.webloc",
            "script.py",
            "thing.jar",
            "run.exe",
            "run.bat",
            "run.ps1",
            "run.vbs",
            "notes.js",
            "notes.jse",
            "launch.jnlp",
            "snapin.msc",
            "setup.application",
            "ref.appref-ms",
            "pkg.appx",
            "pkg.msix",
            "pkg.appinstaller",
            "lib.dylib",
            "agent.desktop",
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, "x").unwrap();
            assert_eq!(openable(&path), Err("not_openable"), "{name}");
        }
        for name in [
            "notes.txt",
            "readme.md",
            "data.json",
            "clip.mp4",
            "bundle.ts",
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, "x").unwrap();
            assert_eq!(openable(&path), Ok(()), "{name}");
        }

        // An execute bit refuses a file with no telling extension.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let runnable = dir.path().join("server");
            std::fs::write(&runnable, "x").unwrap();
            std::fs::set_permissions(&runnable, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(openable(&runnable), Err("not_openable"), "execute bit");
        }

        // A program renamed to a document extension is refused by its header.
        let renamed = dir.path().join("notes.pdf");
        std::fs::write(&renamed, [0xCF, 0xFA, 0xED, 0xFE, 0, 0, 0, 0]).unwrap();
        assert_eq!(openable(&renamed), Err("not_openable"), "Mach-O header");
        let elf = dir.path().join("notes.png");
        std::fs::write(&elf, [0x7F, b'E', b'L', b'F', 0, 0, 0, 0]).unwrap();
        assert_eq!(openable(&elf), Err("not_openable"), "ELF header");
        let fat64 = dir.path().join("notes.md");
        std::fs::write(&fat64, [0xCA, 0xFE, 0xBA, 0xBF, 0, 0, 0, 0]).unwrap();
        assert_eq!(openable(&fat64), Err("not_openable"), "64-bit fat Mach-O");
        // A DOS/PE executable is refused whether its size field reads as text
        // or not: a real one carries the zero fields its header requires.
        let mut pe = b"MZ".to_vec();
        pe.extend_from_slice(b"A\0\x03\x00\x00\x00\x00\x00");
        pe.resize(0x3C, b' ');
        pe.extend_from_slice(&0x80u32.to_le_bytes());
        pe.resize(0x80, b' ');
        pe.extend_from_slice(b"PE\0\0");
        let pe_path = dir.path().join("notes.dat");
        std::fs::write(&pe_path, &pe).unwrap();
        assert_eq!(openable(&pe_path), Err("not_openable"), "PE header");
        let stub = dir.path().join("notes.txt");
        std::fs::write(&stub, b"MZ\x90\x00\x00").unwrap();
        assert_eq!(openable(&stub), Err("not_openable"), "MZ stub");
        // A document that merely starts with the same two letters opens.
        let text = dir.path().join("notes.md");
        std::fs::write(&text, b"MZ is a codec\n").unwrap();
        assert_eq!(openable(&text), Ok(()), "MZ letters in text");
        // And a PE whose header has no zero field at all is still caught by
        // the loader signature its `e_lfanew` points at.
        let at = 0x0101_0101u32;
        let mut crafted = b"MZ".to_vec();
        crafted.extend_from_slice(&[b'A'; 0x3A]);
        crafted.extend_from_slice(&at.to_le_bytes());
        crafted.resize(512, b'A');
        assert!(
            !crafted.contains(&0),
            "the crafted header carries no zero byte"
        );
        let crafted_path = dir.path().join("huge.md");
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&crafted_path).unwrap();
            file.write_all(&crafted).unwrap();
            file.write_all(&vec![b'A'; at as usize - crafted.len()])
                .unwrap();
            file.write_all(b"PE\0\0").unwrap();
        }
        assert_eq!(
            openable(&crafted_path),
            Err("not_openable"),
            "PE by e_lfanew"
        );
    }

    #[test]
    fn token_compare_needs_the_whole_token() {
        assert!(token_matches("abc", "abc"));
        assert!(!token_matches("ab", "abc"));
        assert!(!token_matches("abd", "abc"));
        assert!(!token_matches("", "abc"));
    }

    /// After a launch, a checkout the core learns from Herdr's first sync
    /// reaches a client's snapshot and the boundary on separate reads. A
    /// listing that names it before the boundary applied it is answered once
    /// the roots are current, instead of leaving the Explorer empty on
    /// `outside_checkout`; a root the core does not carry is still refused,
    /// and a refusal for any other reason reads nothing.
    #[test]
    fn an_event_naming_a_root_the_boundary_has_not_applied_yet_is_checked_again_once_the_roots_are_current()
     {
        let home = tempfile::tempdir().unwrap();
        let checkout = home.path().join("checkout");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::write(checkout.join("f65.txt"), "file 65\n").unwrap();
        let boundary = Boundary::new(home.path()).unwrap();
        let root = checkout.display().to_string();

        let mut reads = 0;
        let mut event = json!({"schema_version": 2, "kind": "file_list", "payload": {"root": root, "path": root}});
        let answer = admit_event(&boundary, &mut event, || {
            reads += 1;
            boundary.set_roots(vec![crate::boundary::Root {
                workspace_id: "w1".to_owned(),
                checkout_id: "c1".to_owned(),
                path: checkout.clone(),
            }]);
            true
        })
        .expect("a listing is answered here");
        assert_eq!(reads, 1);
        assert_eq!(answer["type"], "directory_list", "{answer}");
        assert!(answer.to_string().contains("f65.txt"), "{answer}");

        // A root the core does not carry either is refused after one read.
        let other = home.path().join("elsewhere");
        std::fs::create_dir(&other).unwrap();
        let other = other.display().to_string();
        let mut reads = 0;
        let mut event = json!({"schema_version": 2, "kind": "file_list", "payload": {"root": other, "path": other}});
        let answer = admit_event(&boundary, &mut event, || {
            reads += 1;
            true
        })
        .expect("refused");
        assert_eq!(reads, 1);
        assert!(refused_outside_checkout(&answer), "{answer}");

        // A name the boundary refuses for itself never waits on the core.
        let mut event = json!({"schema_version": 2, "kind": "file_create", "payload": {"root": root, "parent": root, "name": "../out"}});
        let answer =
            admit_event(&boundary, &mut event, || panic!("no root was missing")).expect("refused");
        assert_eq!(
            answer["payload"]["reason"],
            Refusal::InvalidPath.code(),
            "{answer}"
        );
    }

    /// A browser display's `file:` address is a path like any other: under a
    /// registered checkout it reaches the core as the URL of the checked
    /// path; outside one, or naming another host, it never does. A web
    /// address is not the boundary's to judge, and a layout action other than
    /// navigate carries no address.
    #[test]
    fn a_file_address_reaches_the_core_only_under_a_checkout() {
        let home = tempfile::tempdir().unwrap();
        let checkout = home.path().join("check out");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::write(checkout.join("보고서.html"), "<p>ok</p>").unwrap();
        std::fs::write(home.path().join("secret.html"), "no").unwrap();
        let boundary = Boundary::new(home.path()).unwrap();
        boundary.set_roots(vec![crate::boundary::Root {
            workspace_id: "w1".to_owned(),
            checkout_id: "c1".to_owned(),
            path: checkout.clone(),
        }]);
        // Sent as `localhost` with the file name escaped in lower case; it
        // reaches the core as the one spelling of the path that was checked.
        let checked =
            crate::file_url::file_url(&checkout.join("보고서.html").display().to_string(), "#top");
        let inside = checked
            .replacen("file://", "file://localhost", 1)
            .replace("%EB%B3%B4", "%eb%b3%b4");

        for kind in ["browser_open", "browser_state"] {
            let mut event = json!({"schema_version": 2, "kind": kind, "payload": {"url": inside}});
            assert_eq!(apply_boundary(&boundary, &mut event), None, "{kind}");
            assert_eq!(event["payload"]["url"], checked, "{kind}");
        }
        let mut event = json!({"schema_version": 2, "kind": "view_layout", "payload": {"action": "navigate", "display_id": "d1", "url": inside}});
        assert_eq!(apply_boundary(&boundary, &mut event), None);
        assert_eq!(event["payload"]["url"], checked);

        let outside =
            crate::file_url::file_url(&home.path().join("secret.html").display().to_string(), "");
        for (url, reason) in [
            (outside.as_str(), Refusal::OutsideCheckout),
            ("file://server/share/a.html", Refusal::InvalidPath),
        ] {
            let mut event =
                json!({"schema_version": 2, "kind": "browser_open", "payload": {"url": url}});
            let answer = apply_boundary(&boundary, &mut event).expect("refused");
            assert_eq!(answer["type"], "path_refused", "{answer}");
            assert_eq!(answer["payload"]["reason"], reason.code(), "{answer}");
        }
        let mut event = json!({"schema_version": 2, "kind": "view_layout", "payload": {"action": "navigate", "display_id": "d1", "url": outside}});
        assert!(apply_boundary(&boundary, &mut event).is_some());

        for mut event in [
            json!({"schema_version": 2, "kind": "browser_open", "payload": {"url": "https://example.com/a"}}),
            json!({"schema_version": 2, "kind": "view_layout", "payload": {"action": "close", "display_id": "d1", "url": outside}}),
        ] {
            let before = event.clone();
            assert_eq!(apply_boundary(&boundary, &mut event), None);
            assert_eq!(event, before);
        }
    }
}
