use std::collections::HashSet;
use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};
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
    /// The daemon's one watch service; every client subscribes to its frames.
    pub watch: Arc<WatchService>,
    /// The ⌘P index cache, one lazy index per registered checkout.
    pub index: Arc<IndexService>,
    /// Staged dropped files on their way to the core's attachment directory.
    pub attachments: Arc<Attachments>,
    pub token: Arc<String>,
    pub allowed_origins: Arc<HashSet<String>>,
    pub clients: Arc<AtomicUsize>,
    /// Numbers connections so a stage can be released with its connection.
    pub connections: Arc<AtomicU64>,
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
    // A change in a watched folder is announced on this socket beside the
    // snapshot stream; the client re-reads the one folder it names (B2).
    let mut directory_changes = state.watch.subscribe();
    if send_snapshot(&mut socket, &state, &mut have_revision, &mut have_sequence)
        .await
        .is_err()
    {
        client_gone(&state, connection);
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
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let replies = match handle_client_text(&state, &text, connection).await {
                            Ok(frames) => frames,
                            Err(error) => vec![Message::Text(
                                json!({"type":"error","payload":{},"message": error}).to_string().into(),
                            )],
                        };
                        for frame in replies {
                            if socket.send(frame).await.is_err() {
                                break;
                            }
                        }
                    }                    Some(Ok(Message::Binary(bytes))) => {
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
    client_gone(&state, connection);
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
async fn handle_client_text(
    state: &AppState,
    text: &str,
    connection: u64,
) -> Result<Vec<Message>, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("client json: {error}"))?;
    let mut event = if value.get("schema_version").is_some() && value.get("kind").is_some() {
        value
    } else {
        return Err("expected a core event {schema_version, kind, payload}".to_owned());
    };
    if event.get("kind").and_then(Value::as_str) == Some("file_bytes") {
        return Ok(handle_file_bytes(&state.boundary, &event).await);
    }
    if event.get("kind").and_then(Value::as_str) == Some("file_index") {
        return Ok(handle_file_index(state, &event));
    }
    match event.get("kind").and_then(Value::as_str) {
        Some("attachment_stage") => return Ok(handle_attachment_stage(state, &event, connection)),
        Some("attachment_commit") => return Ok(handle_attachment_commit(state, &event)),
        Some("attachment_cancel") => {
            state
                .attachments
                .discard(&payload_str(&event, "request_id"));
            return Ok(Vec::new());
        }
        Some("open_external") => return Ok(handle_open_external(state, &event)),
        _ => {}
    }
    if let Some(reply) = apply_boundary(&state.boundary, &mut event) {
        return Ok(vec![Message::Text(reply.to_string().into())]);
    }
    let bytes = serde_json::to_vec(&event).map_err(|error| format!("event encode: {error}"))?;
    state.core.dispatch(bytes).map(|()| Vec::new())
}

/// Opens a checkout file with the host OS handler, or with the program
/// `HIDE_OPEN_COMMAND` names (PRD S3 D-12). The path passes the same
/// checkout-root boundary as every other Explorer path, so the handler can
/// only ever be pointed at a regular file inside a registered root.
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
                "path": real.display().to_string(),
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
    let (ok, reason) = match open_with_handler(&real) {
        Ok(()) => (true, Value::Null),
        Err(reason) => (false, Value::String(reason.to_owned())),
    };
    eprintln!(
        "{}",
        json!({
            "component": "hided",
            "kind": "open.external",
            "ok": ok,
            "path": real.display().to_string(),
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

/// The program that opens one file: the configured one, else the host's.
fn opener(path: &Path) -> Command {
    let configured = std::env::var(crate::env::HIDE_OPEN_COMMAND)
        .ok()
        .filter(|value| !value.is_empty());
    let mut command = match configured {
        Some(program) => Command::new(program),
        None => platform_opener(),
    };
    command.arg(path);
    command
}

#[cfg(target_os = "macos")]
fn platform_opener() -> Command {
    Command::new("open")
}

#[cfg(target_os = "windows")]
fn platform_opener() -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "start", ""]);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_opener() -> Command {
    Command::new("xdg-open")
}

/// Spawns the handler detached: the daemon never waits for an application to
/// exit, and the handler's own output is not the shell's. A dropped child
/// would stay a zombie for the daemon's life, so one thread reaps it.
fn open_with_handler(path: &Path) -> Result<(), &'static str> {
    let mut child = opener(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "spawn_failed")?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
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
    let Some(known) = state.boundary.known_root(&root) else {
        return vec![Message::Text(
            refused("file_index", &root, Refusal::OutsideCheckout)
                .to_string()
                .into(),
        )];
    };
    let root_path = known.display().to_string();
    let payload = match state.index.query(&known, &query) {
        IndexAnswer::Indexing => json!({
            "root_path": root_path,
            "query": query,
            "files": [],
            "truncated": false,
            "indexing": true,
        }),
        IndexAnswer::Ready { entries, truncated } => {
            let listed: Vec<Value> = entries
                .iter()
                .map(|relative| {
                    json!({
                        "path": known.join(relative).display().to_string(),
                        "relative_path": relative,
                    })
                })
                .collect();
            json!({
                "root_path": root_path,
                "query": query,
                "files": listed,
                "truncated": truncated,
                "indexing": false,
            })
        }
    };
    vec![Message::Text(
        json!({"type": "file_index_result", "payload": payload})
            .to_string()
            .into(),
    )]
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

/// A `file_bytes` read: the path is checked against the checkout roots, the
/// cap is applied, and the requested range is streamed as binary frames. A
/// boundary refusal is the same `path_refused` frame every other path gets.
async fn handle_file_bytes(boundary: &Boundary, event: &Value) -> Vec<Message> {
    let request_id = payload_str(event, "request_id");
    let path = payload_str(event, "path");
    let offset = event
        .pointer("/payload/offset")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let length = event.pointer("/payload/length").and_then(Value::as_u64);
    let (real, total) = match boundary.resolve_file(&path) {
        Ok(pair) => pair,
        Err(refusal) => {
            return vec![Message::Text(
                refused("file_bytes", &path, refusal).to_string().into(),
            )];
        }
    };
    let wanted = length.unwrap_or_else(|| total.saturating_sub(offset.min(total)));
    if wanted > boundary::MAX_FILE_BYTES {
        return vec![file_bytes_error(&request_id, &path, "too_large")];
    }
    let mut file = match tokio::fs::File::open(&real).await {
        Ok(file) => file,
        Err(_) => return vec![file_bytes_error(&request_id, &path, "read_failed")],
    };
    let start = offset.min(total);
    if start > 0 && file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return vec![file_bytes_error(&request_id, &path, "read_failed")];
    }
    let end = start.saturating_add(wanted).min(total);
    let displayed = real.display().to_string();
    let mut frames = Vec::new();
    let mut cursor = start;
    loop {
        let take = (end - cursor).min(BYTES_CHUNK) as usize;
        let mut buffer = vec![0u8; take];
        let read = match file.read(&mut buffer).await {
            Ok(read) => read,
            Err(_) => {
                frames.push(file_bytes_error(&request_id, &path, "read_failed"));
                return frames;
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
        frames.push(Message::Binary(bytes_frame(&header, &buffer).into()));
        cursor += read as u64;
        if eof {
            return frames;
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

fn client_gone(state: &AppState, connection: u64) {
    state.attachments.release(connection);
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
    fn the_platform_opener_is_the_hosts_own() {
        #[cfg(target_os = "macos")]
        assert_eq!(platform_opener().get_program(), "open");
        #[cfg(target_os = "windows")]
        assert_eq!(platform_opener().get_program(), "cmd");
        #[cfg(all(unix, not(target_os = "macos")))]
        assert_eq!(platform_opener().get_program(), "xdg-open");
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
}
