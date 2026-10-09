//! This daemon on a screen machine whose core runs elsewhere (PRD
//! core-host-node-remote-core D-02, D-05, D-06, D-08): it starts no core.
//! It keeps its node's link to the core ([`NodeRole`]) and serves its own
//! screens on loopback exactly as a core's daemon does, `/ws` included, so
//! the screen and the desktop host cannot tell the difference.
//!
//! Each screen's core traffic (events, snapshots and deltas, answers) goes
//! to the core through one relay per screen on the link's SSH connection.
//! Terminals are drawn from this daemon's own hub: this machine's panes
//! from its own terminals without the link, every other pane from the one
//! terminals relay the node keeps to the core. So a key into one of this
//! machine's panes and its output never leave the machine.
//!
//! Each screen's relay is read as fast as its core sends, whatever the
//! screen takes: a screen that stalls holds at most 4 MiB of core frames,
//! and past that it is closed as fallen behind and reattaches (D-20, B17),
//! so it never stops the SSH connection every other screen, pane and the
//! link share.
//!
//! Keys and redraws for the core's panes wait, in order, while the
//! terminals relay is not there to take them (it opens after the link, and
//! again after it fails): at most 64 KiB, each at most 3 s, as the core
//! holds keys typed before a pane exists. A key past the bound is refused
//! to the screen that typed it, and one that waited too long is dropped and
//! that screen told; none is lost unsaid.
//!
//! While the link is down a screen is held: its socket stays open, every
//! frame it sends is dropped and counted, and nothing reaches it until the
//! link is back; a screen that was attached when the link ended is closed
//! once, so its next attempt is the held one (amendment 4 of the plan's
//! review). Keys typed meanwhile are never delivered later (B8).

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures_util::{SinkExt, StreamExt};
use hide_node::terminal::OutputSink;
use hide_node_link::terminal::{
    KeyTarget, MAX_PANE_ID_BYTES, TerminalDown, TerminalLine, TerminalNode, TerminalUp,
    decode_base64, device_pane_prefix, encode_base64,
};
use serde_json::{Value, json};
use tokio::sync::{Notify, mpsc, watch};
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::attachments::Attachments;
use crate::boundary::{Boundary, Refusal, Root};
use crate::browser_routes::{BrowserRoutes, RouteRequest};
use crate::node_browser::NodeBrowser;
use crate::node_pages::NodePages;
use crate::node_role::{LiveLink, NodeIdentity, NodeRole, Phase};
use crate::node_uploads::{Handled, ScreenUploads};
use crate::placement::Placement;
use crate::server::{
    CloseReason, FIRST_FRAME_TIMEOUT, Handshake, RELAY_GRANT_HEADER, check_origin, refuse,
    terminal_key,
};
use crate::state_file::{MAX_CLIENTS, SCHEMA_VERSION};
use crate::terminal_hub::{HubClient, Resume, TerminalHub};

/// Terminal lines for the core's panes waiting to go up the terminals
/// relay, in bytes; past it a key is refused to its screen.
const HELD_BYTES: usize = 64 * 1024;
/// How long one such line may wait; past it it is dropped and its screen
/// told.
const HELD_FOR: Duration = Duration::from_secs(3);
/// How long a terminals relay that failed waits before it opens again on
/// the same link.
const RELAY_RETRY: Duration = Duration::from_secs(1);
/// The largest message a relay of the core may send this daemon.
const RELAY_MAX_MESSAGE: usize = 16 * 1024 * 1024;
/// Core frames one screen may leave untaken before its backlog is dropped
/// and it is drawn again from a fresh snapshot (D-20).
const SCREEN_BACKLOG_BYTES: usize = 4 * 1024 * 1024;
/// A screen's frames waiting to go up its relay; past it the screen's
/// socket is read no further until the relay takes them.
const SCREEN_TO_CORE: usize = 64;

/// What the node role's screen server holds.
#[derive(Clone)]
pub struct NodeState {
    pub token: Arc<String>,
    pub allowed_origins: Arc<HashSet<String>>,
    pub ui_dir: Option<PathBuf>,
    pub version: &'static str,
    pub build: Option<Arc<str>>,
    pub hub: Arc<TerminalHub>,
    pub terminals: Arc<ScreenTerminals>,
    pub live: watch::Receiver<Option<Arc<LiveLink>>>,
    pub phase: Arc<dyn Fn() -> Phase + Send + Sync>,
    pub clients: Arc<AtomicUsize>,
    pub connections: Arc<AtomicU64>,
    pub last_client_gone: Arc<Mutex<Instant>>,
    pub shutdown: Arc<Notify>,
    /// Frames screens sent while the link was down, dropped.
    pub held_frames: Arc<AtomicU64>,
    /// This machine's paths a screen may read without the core: the
    /// checkouts the core opened on this node over the live link.
    pub boundary: Arc<Boundary>,
    /// The Browser View routes this machine's desktop host resolves
    /// (`node_pages`).
    pub browser_routes: Arc<BrowserRoutes>,
    /// Desktop windows attached now; with none, every route is closed.
    pub desktop_screens: Arc<AtomicUsize>,
    /// Files this machine's screens paste or drop, staged here
    /// (`node_uploads`).
    pub attachments: Arc<Attachments>,
    /// This machine's desktop windows' browser gateways (`node_browser`).
    pub browser: Arc<NodeBrowser>,
}

/// This machine's panes, as its hub names them: the core's names for them.
struct OwnPanes {
    hub: Arc<TerminalHub>,
    prefix: String,
}

impl OutputSink for OwnPanes {
    fn output(&self, pane: &str, bytes: &[u8], full: bool) {
        self.hub
            .output(&format!("{}{pane}", self.prefix), bytes, full);
    }

    fn forget(&self, pane: &str) {
        self.hub.forget(&format!("{}{pane}", self.prefix));
    }
}

/// Where a screen's keys and redraws go: this machine's panes to
/// its own terminals for the link's life, every other pane up the
/// terminals relay.
pub struct ScreenTerminals {
    own_prefix: String,
    live: watch::Receiver<Option<Arc<LiveLink>>>,
    held: Mutex<HeldLines>,
    /// Wakes the terminals relay's writer when a line is held.
    ready: Notify,
    /// The screen connection whose held key was dropped as too old.
    dropped: tokio::sync::broadcast::Sender<u64>,
}

/// Lines for the core's panes not yet written up the terminals relay, in
/// the order the screens sent them.
#[derive(Default)]
struct HeldLines {
    lines: VecDeque<HeldLine>,
    bytes: usize,
}

struct HeldLine {
    text: String,
    connection: u64,
    at: Instant,
}

impl HeldLines {
    /// The next line still in time, telling `dropped` of each that is not.
    fn next(
        &mut self,
        now: Instant,
        dropped: &tokio::sync::broadcast::Sender<u64>,
    ) -> Option<String> {
        while let Some(line) = self.lines.pop_front() {
            self.bytes -= line.text.len();
            if now.duration_since(line.at) <= HELD_FOR {
                return Some(line.text);
            }
            expired(line.connection, dropped);
        }
        None
    }

    /// Drops every line older than [`HELD_FOR`], telling its screen.
    fn expire(&mut self, now: Instant, dropped: &tokio::sync::broadcast::Sender<u64>) {
        while self
            .lines
            .front()
            .is_some_and(|line| now.duration_since(line.at) > HELD_FOR)
        {
            if let Some(line) = self.lines.pop_front() {
                self.bytes -= line.text.len();
                expired(line.connection, dropped);
            }
        }
    }

    /// Drops every line: the link they were typed for ended.
    fn clear(&mut self, dropped: &tokio::sync::broadcast::Sender<u64>) {
        for line in self.lines.drain(..) {
            expired(line.connection, dropped);
        }
        self.bytes = 0;
    }
}

fn expired(connection: u64, dropped: &tokio::sync::broadcast::Sender<u64>) {
    herdr_core::diagnostic!(json!({
        "component": "node_daemon",
        "kind": "terminals.held_dropped",
        "connection": connection,
        "held_ms": HELD_FOR.as_millis() as u64,
    }));
    let _ = dropped.send(connection);
}

impl ScreenTerminals {
    fn new(own_prefix: String, live: watch::Receiver<Option<Arc<LiveLink>>>) -> Self {
        Self {
            own_prefix,
            live,
            held: Mutex::new(HeldLines::default()),
            ready: Notify::new(),
            dropped: tokio::sync::broadcast::channel(64).0,
        }
    }

    fn own(&self) -> Option<Arc<LiveLink>> {
        self.live.borrow().clone()
    }

    /// Holds `line` for the terminals relay, behind every line held before
    /// it; refused past [`HELD_BYTES`]. One line is always held, however
    /// large, so a paste goes up as it would on the core's own screen.
    fn up(&self, line: TerminalDown, connection: u64) -> Result<(), String> {
        let text = serde_json::to_string(&TerminalLine { terminal: line })
            .map_err(|error| error.to_string())?;
        let mut held = lock(&self.held);
        if !held.lines.is_empty() && held.bytes + text.len() > HELD_BYTES {
            drop(held);
            herdr_core::diagnostic!(json!({
                "component": "node_daemon",
                "kind": "terminals.held_full",
                "connection": connection,
                "cap": HELD_BYTES,
            }));
            return Err(
                "The core's panes are not taking keys now; this key was not sent".to_owned(),
            );
        }
        held.bytes += text.len();
        held.lines.push_back(HeldLine {
            text,
            connection,
            at: Instant::now(),
        });
        drop(held);
        self.ready.notify_one();
        Ok(())
    }

    /// A key a screen typed: into this machine's pane, or held for the
    /// core's.
    fn key(
        &self,
        connection: u64,
        target: KeyTarget,
        bytes: Vec<u8>,
        typed_at_unix_ms: u64,
    ) -> Result<(), String> {
        if let KeyTarget::Pane(pane) = &target
            && let Some(own) = pane.strip_prefix(&self.own_prefix)
        {
            if let Some(link) = self.own() {
                link.terminals
                    .key(KeyTarget::Pane(own.to_owned()), bytes, typed_at_unix_ms);
            }
            return Ok(());
        }
        self.up(
            TerminalDown::Key {
                target,
                data: encode_base64(&bytes),
                typed_at_unix_ms,
            },
            connection,
        )
    }

    /// A screen's view of `pane` needs it drawn whole.
    fn redraw(&self, connection: u64, pane: &str) {
        if let Some(own) = pane.strip_prefix(&self.own_prefix) {
            if let Some(link) = self.own() {
                link.terminals.redraw(own);
            }
            return;
        }
        // A redraw refused is asked again by the screen's next view.
        let _ = self.up(
            TerminalDown::Redraw {
                pane: pane.to_owned(),
            },
            connection,
        );
    }
}

/// The node role, running: its link and its screen server.
pub struct NodeDaemon {
    pub state: NodeState,
    role: Arc<NodeRole>,
    relay: tokio::task::JoinHandle<()>,
    reaper: tokio::task::JoinHandle<()>,
    browser: tokio::task::JoinHandle<()>,
}

impl NodeDaemon {
    /// Starts the link to the core `placement` names and the terminals
    /// relay that follows it; the caller serves [`router`] with `state`.
    pub fn start(
        home: &std::path::Path,
        placement: Placement,
        identity: NodeIdentity,
        server: ServerParts,
    ) -> Result<Self, String> {
        let hub = TerminalHub::new();
        let own_prefix = device_pane_prefix(&identity.node);
        let screen_node = identity.node.clone();
        let core_node = placement.node.clone();
        let boundary = Arc::new(Boundary::for_node(
            home,
            herdr_core::node::NodeId::parse(&identity.node)?,
        )?);
        let browser = NodeBrowser::new(server.port);
        let role = Arc::new(NodeRole::start_with_browser(
            home,
            placement,
            identity,
            Arc::new(OwnPanes {
                hub: Arc::clone(&hub),
                prefix: own_prefix.clone(),
            }),
            Some(Arc::clone(&browser)),
        )?);
        let live = role.live();
        let follow = browser.spawn_follow(live.clone());
        let terminals = Arc::new(ScreenTerminals::new(own_prefix.clone(), live.clone()));
        let relay = tokio::spawn(keep_terminals_relay(
            live.clone(),
            Arc::clone(&hub),
            Arc::clone(&terminals),
        ));
        // The screen server reads the phase without keeping the role: the
        // role ends with this daemon, whatever the server still holds.
        let phase = {
            let role = Arc::downgrade(&role);
            Arc::new(move || {
                role.upgrade().map_or_else(
                    || Phase::Waiting {
                        reason: "stopping".to_owned(),
                    },
                    |role| role.phase(),
                )
            }) as Arc<dyn Fn() -> Phase + Send + Sync>
        };
        let browser_routes = BrowserRoutes::new(Arc::new(NodePages::new(
            screen_node,
            core_node,
            live.clone(),
            Arc::clone(&boundary),
        )));
        let desktop_screens = Arc::new(AtomicUsize::new(0));
        let reaper =
            browser_routes.spawn_reaper(Arc::clone(&desktop_screens), Arc::clone(&server.shutdown));
        Ok(Self {
            state: NodeState {
                token: server.token,
                allowed_origins: server.allowed_origins,
                ui_dir: server.ui_dir,
                version: server.version,
                build: server.build,
                hub,
                terminals,
                live,
                phase,
                clients: Arc::new(AtomicUsize::new(0)),
                connections: Arc::new(AtomicU64::new(0)),
                last_client_gone: Arc::new(Mutex::new(Instant::now())),
                boundary,
                shutdown: server.shutdown,
                held_frames: Arc::new(AtomicU64::new(0)),
                browser_routes,
                desktop_screens,
                attachments: Arc::new(Attachments::new(&server.state_dir)),
                browser,
            },
            role,
            relay,
            reaper,
            browser: follow,
        })
    }

    /// The link's phase, for a caller that waits on it.
    pub fn role(&self) -> &NodeRole {
        &self.role
    }
}

impl Drop for NodeDaemon {
    fn drop(&mut self) {
        self.relay.abort();
        self.reaper.abort();
        self.browser.abort();
    }
}

/// What the screen server is given by the daemon that starts it.
pub struct ServerParts {
    pub token: Arc<String>,
    pub allowed_origins: Arc<HashSet<String>>,
    pub ui_dir: Option<PathBuf>,
    pub version: &'static str,
    pub build: Option<Arc<str>>,
    pub shutdown: Arc<Notify>,
    /// This machine's state folder, where its screens' uploads are staged.
    pub state_dir: PathBuf,
    /// The loopback port the screen server listens on.
    pub port: u16,
}

pub fn router(state: NodeState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_upgrade))
        .route(
            "/browser-route",
            post(resolve_browser_route).delete(release_browser_route),
        )
        .route(
            "/browser-control",
            post(crate::node_browser::register)
                .delete(crate::node_browser::release)
                .layer(axum::extract::DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/browser-control/action",
            post(crate::node_browser::action)
                .layer(axum::extract::DefaultBodyLimit::max(16 * 1024)),
        )
        .route("/browser-relay/{ticket}", get(crate::node_browser::relay))
        .route("/", get(static_asset))
        .route("/assets/{*path}", get(static_asset))
        .fallback(|| async { axum::http::StatusCode::NOT_FOUND })
        .with_state(state)
}

/// What `hide connect` reads, as a core's daemon answers it, and where the
/// link to the core stands.
async fn health(State(state): State<NodeState>) -> impl IntoResponse {
    let phase = (state.phase)();
    let (link, reason) = match &phase {
        Phase::Connecting => ("connecting", None),
        Phase::Live(_) => ("live", None),
        Phase::Waiting { reason } => ("waiting", Some(reason.clone())),
    };
    axum::Json(json!({
        "pid": std::process::id(),
        "version": state.version,
        "build": state.build.as_deref(),
        "schema_version": SCHEMA_VERSION,
        "clients": state.clients.load(Ordering::SeqCst),
        "open_handlers_in_flight": 0,
        "idle_remaining_secs": Value::Null,
        "role": "node",
        "node": state.boundary.node().as_str(),
        "core_link": link,
        "core_link_reason": reason,
    }))
}

async fn resolve_browser_route(
    State(state): State<NodeState>,
    headers: HeaderMap,
    axum::Json(request): axum::Json<RouteRequest>,
) -> Response {
    if !crate::server::bearer_matches(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    crate::browser_routes::resolve_answer(&state.browser_routes, request).await
}

async fn release_browser_route(
    State(state): State<NodeState>,
    headers: HeaderMap,
    axum::Json(request): axum::Json<RouteRequest>,
) -> Response {
    if !crate::server::bearer_matches(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    crate::browser_routes::release_answer(&state.browser_routes, request).await
}

async fn static_asset(uri: Uri, State(state): State<NodeState>) -> Response {
    crate::server::ui_asset(state.ui_dir.as_deref(), uri.path()).await
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<NodeState>,
) -> Response {
    let origin = headers
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    // A screen of this machine only: the node role serves no phone and
    // nothing through `tailscale serve`.
    let proxied = crate::server::via_tailnet(&headers);
    ws.on_upgrade(move |socket| screen(socket, state, origin, proxied))
}

async fn screen(mut socket: WebSocket, state: NodeState, origin: Option<String>, proxied: bool) {
    if proxied || check_origin(origin.as_deref(), &state.allowed_origins).is_err() {
        refuse(&mut socket, CloseReason::OriginNotAllowed, None).await;
        return;
    }
    let handshake_text = match tokio::time::timeout(FIRST_FRAME_TIMEOUT, socket.recv()).await {
        Ok(Some(Ok(Message::Text(text)))) => text.to_string(),
        _ => {
            refuse(&mut socket, CloseReason::InvalidToken, None).await;
            return;
        }
    };
    let Ok(handshake) = serde_json::from_str::<Handshake>(&handshake_text) else {
        refuse(&mut socket, CloseReason::InvalidToken, None).await;
        return;
    };
    if handshake.schema_version != SCHEMA_VERSION {
        refuse(&mut socket, CloseReason::SchemaMismatch, None).await;
        return;
    }
    if !crate::server::token_matches(&handshake.token, &state.token) {
        refuse(&mut socket, CloseReason::InvalidToken, None).await;
        return;
    }
    let previous = state.clients.fetch_add(1, Ordering::SeqCst);
    if previous >= MAX_CLIENTS {
        state.clients.fetch_sub(1, Ordering::SeqCst);
        refuse(&mut socket, CloseReason::ClientLimit, Some(previous + 1)).await;
        return;
    }
    let connection = state.connections.fetch_add(1, Ordering::SeqCst);
    let desktop = handshake.client_kind.as_deref() == Some("desktop");
    if desktop {
        state.desktop_screens.fetch_add(1, Ordering::SeqCst);
    }
    let mut local_reads = 0_u64;
    let ended = attached(
        &mut socket,
        &state,
        handshake,
        &forwarded_handshake(&handshake_text),
        connection,
        &mut local_reads,
    )
    .await;
    if desktop {
        state.desktop_screens.fetch_sub(1, Ordering::SeqCst);
    }
    herdr_core::diagnostic!(json!({
        "component": "node_daemon",
        "kind": "screen.ended",
        "connection": connection,
        "reason": ended.reason(),
        "local_file_reads": local_reads,
    }));
    if let Some(close) = ended.close() {
        let _ = socket.send(Message::Close(Some(close))).await;
    }
    let remaining = state
        .clients
        .fetch_sub(1, Ordering::SeqCst)
        .saturating_sub(1);
    if remaining == 0 {
        *state
            .last_client_gone
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Instant::now();
    }
}

type Upstream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Opens a relay of `mode` to the core through the link's forward.
async fn open_relay(link: &LiveLink, mode: &str) -> Result<Upstream, String> {
    let address = SocketAddr::from(([127, 0, 0, 1], link.relay_port));
    let mut request = format!("ws://{address}/relay?mode={mode}")
        .into_client_request()
        .map_err(|error| error.to_string())?;
    request.headers_mut().insert(
        RELAY_GRANT_HEADER,
        link.accepted
            .relay_token
            .parse()
            .map_err(|_| "the relay grant is not a header value".to_owned())?,
    );
    let config = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(RELAY_MAX_MESSAGE))
        .max_frame_size(Some(RELAY_MAX_MESSAGE));
    let (socket, _) = tokio::time::timeout(
        Duration::from_secs(15),
        tokio_tungstenite::connect_async_with_config(request, Some(config), true),
    )
    .await
    .map_err(|_| "the relay did not open in time".to_owned())?
    .map_err(|error| error.to_string())?;
    Ok(socket)
}

/// Waits for a live link, holding the screen: what it sends meanwhile is
/// dropped and counted. `None` when the screen left first.
async fn hold(socket: &mut WebSocket, state: &NodeState) -> Option<Arc<LiveLink>> {
    let mut live = state.live.clone();
    loop {
        if let Some(link) = live.borrow_and_update().clone() {
            return Some(link);
        }
        tokio::select! {
            changed = live.changed() => {
                if changed.is_err() {
                    return None;
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return None,
                Some(Ok(_)) => {
                    state.held_frames.fetch_add(1, Ordering::Relaxed);
                }
            },
        }
    }
}

/// One screen while the link stands: its core traffic through its relay,
/// its terminals from this daemon's hub. Answers why it ended.
async fn attached(
    socket: &mut WebSocket,
    state: &NodeState,
    handshake: Handshake,
    handshake_text: &str,
    connection: u64,
    local_reads: &mut u64,
) -> ScreenEnd {
    let Some(link) = hold(socket, state).await else {
        return ScreenEnd::Left;
    };
    let relay = tokio::select! {
        opened = ScreenRelay::open(&link, handshake_text, connection) => opened,
        // What the screen sends before its relay stands is dropped like
        // what it sends while the link is down, never delivered late (B8).
        () = drop_until_closed(socket, state) => return ScreenEnd::Left,
    };
    let relay = match relay {
        Ok(relay) => relay,
        Err(message) => {
            herdr_core::diagnostic!(json!({
                "component": "node_daemon",
                "kind": "screen.relay_failed",
                "connection": connection,
                "message": message,
            }));
            return ScreenEnd::LinkLost;
        }
    };
    let mut live = state.live.clone();
    let mut terminals: Option<HubClient> = None;
    let mut told = InputNotices::default();
    let mut dropped = state.terminals.dropped.subscribe();
    let mut uploads = ScreenUploads::new(
        Arc::clone(&state.attachments),
        connection,
        state.boundary.node().as_str(),
        &state.terminals.own_prefix,
    );
    loop {
        let shared = Arc::clone(&relay.shared);
        tokio::select! {
            changed = live.changed() => {
                let same = changed.is_ok()
                    && live
                        .borrow_and_update()
                        .as_ref()
                        .is_some_and(|now| now.generation == link.generation);
                if !same {
                    return ScreenEnd::LinkLost;
                }
            }
            () = async {
                match &terminals {
                    Some(client) => client.ready().await,
                    None => std::future::pending().await,
                }
            } => {
                let Some(client) = &terminals else { continue };
                let (frame, redraws) = client.take();
                for pane in redraws {
                    state.terminals.redraw(connection, &pane);
                }
                if let Some(frame) = frame
                    && socket.send(Message::Text(frame.text.into())).await.is_err()
                {
                    return ScreenEnd::Left;
                }
            }
            missed = dropped.recv() => {
                if matches!(missed, Ok(missed) if missed == connection) {
                    let frame = json!({
                        "type": "error",
                        "payload": {},
                        "message": "A key for one of the core's panes waited too long and was not sent",
                    });
                    if socket.send(Message::Text(frame.to_string().into())).await.is_err() {
                        return ScreenEnd::Left;
                    }
                }
            }
            () = shared.ready.notified() => {
                loop {
                    let next = lock(&shared.backlog).pop();
                    let Some(message) = next else { break };
                    match message {
                        tungstenite::Message::Text(text) => {
                            let kind = frame_kind(&text);
                            match (kind, &terminals) {
                                (Some(kind), None) => {
                                    terminals = Some(state.hub.connect(resume(kind, &handshake)));
                                }
                                (Some(FrameStart::Snapshot), Some(client)) => client.restart(),
                                _ => {}
                            }
                            if socket.send(Message::Text(text.as_str().into())).await.is_err() {
                                return ScreenEnd::Left;
                            }
                        }
                        tungstenite::Message::Binary(bytes) => {
                            if socket.send(Message::Binary(bytes)).await.is_err() {
                                return ScreenEnd::Left;
                            }
                        }
                        _ => {}
                    }
                }
                let end = lock(&shared.backlog).end.take();
                match end {
                    None => {}
                    Some(RelayEnd::Closed(None)) => return ScreenEnd::LinkLost,
                    Some(RelayEnd::Closed(Some(refusal))) => return ScreenEnd::Refused(refusal),
                    // The screen fell behind and what it missed is gone,
                    // answers to its reads among them: it is closed, so it
                    // fails what it waits for and reattaches from what it
                    // last applied.
                    Some(RelayEnd::Overflowed) => return ScreenEnd::FellBehind,
                }
            }
            from_screen = socket.recv() => {
                let message = match from_screen {
                    Some(Ok(message)) => message,
                    _ => return ScreenEnd::Left,
                };
                match message {
                    Message::Text(text) => {
                        if let Some((event, opened)) = own_file(state, &link, &text) {
                            *local_reads += 1;
                            if crate::server::send_opened_file_bytes(socket, &event, opened).await.is_err() {
                                return ScreenEnd::Left;
                            }
                            continue;
                        }
                        match uploads.text(&text).await {
                            Handled::NotUpload => {}
                            Handled::Answer(frames) => {
                                for frame in frames {
                                    if socket.send(Message::Text(frame.to_string().into())).await.is_err() {
                                        return ScreenEnd::Left;
                                    }
                                }
                                continue;
                            }
                            Handled::Up(frames) => {
                                for frame in frames {
                                    if relay.up(frame).await.is_err() {
                                        return ScreenEnd::LinkLost;
                                    }
                                }
                                continue;
                            }
                        }
                        if let Some(reply) = take_terminal_event(state, connection, &text) {
                            match reply {
                                Ok(Some(pane)) => {
                                    if let Some(notice) = told.notice(&pane, Instant::now())
                                        && relay.up(tungstenite::Message::Text(notice.into())).await.is_err()
                                    {
                                        return ScreenEnd::LinkLost;
                                    }
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    let frame = json!({"type":"error","payload":{},"message": error});
                                    if socket.send(Message::Text(frame.to_string().into())).await.is_err() {
                                        return ScreenEnd::Left;
                                    }
                                }
                            }
                            continue;
                        }
                        if relay.up(tungstenite::Message::Text(text.as_str().into())).await.is_err() {
                            return ScreenEnd::LinkLost;
                        }
                    }
                    // Every binary frame a screen sends is an upload chunk,
                    // staged on this machine (`node_uploads`).
                    Message::Binary(bytes) => {
                        for frame in uploads.chunk(&bytes) {
                            if socket.send(Message::Text(frame.to_string().into())).await.is_err() {
                                return ScreenEnd::Left;
                            }
                        }
                    }
                    Message::Close(_) => return ScreenEnd::Left,
                    _ => {}
                }
            }
        }
    }
}

/// Why a screen's relay gives no more frames.
#[derive(Clone, Debug, Eq, PartialEq)]
enum RelayEnd {
    /// The core or the link closed it; with the core's refusal of the
    /// screen (4001 to 4004), which the screen is told as is.
    Closed(Option<CloseFrame>),
    /// The screen left more than [`SCREEN_BACKLOG_BYTES`] untaken.
    Overflowed,
}

/// The core frames one screen's relay read that the screen has not taken.
#[derive(Default)]
struct Backlog {
    frames: VecDeque<tungstenite::Message>,
    bytes: usize,
    end: Option<RelayEnd>,
}

impl Backlog {
    /// Holds `frame`, or, past the cap, drops every frame held and ends the
    /// relay. One frame is always held, however large (a file read sends
    /// 4 MiB at a time), so only a screen that leaves frames untaken falls
    /// behind. Answers whether the relay goes on.
    fn push(&mut self, frame: tungstenite::Message) -> bool {
        if self.end.is_some() {
            return false;
        }
        if !self.frames.is_empty() && self.bytes + frame.len() > SCREEN_BACKLOG_BYTES {
            self.frames.clear();
            self.bytes = 0;
            self.end = Some(RelayEnd::Overflowed);
            return false;
        }
        self.bytes += frame.len();
        self.frames.push_back(frame);
        true
    }

    fn pop(&mut self) -> Option<tungstenite::Message> {
        let frame = self.frames.pop_front()?;
        self.bytes -= frame.len();
        Some(frame)
    }

    fn end(&mut self, end: RelayEnd) {
        self.end.get_or_insert(end);
    }
}

#[derive(Default)]
struct RelayShared {
    backlog: Mutex<Backlog>,
    ready: Notify,
}

/// One screen's relay to its core. Its task reads the core whatever the
/// screen takes: russh stops reading the whole SSH connection while one
/// channel's buffer is full, so a relay left unread would stall every other
/// screen, the panes and the link.
struct ScreenRelay {
    shared: Arc<RelayShared>,
    to_core: mpsc::Sender<tungstenite::Message>,
    task: tokio::task::JoinHandle<()>,
}

impl ScreenRelay {
    /// The relay of the screen `connection`, opened with `handshake`.
    async fn open(link: &LiveLink, handshake: &str, connection: u64) -> Result<Self, String> {
        let mut upstream = open_relay(link, "screen").await?;
        upstream
            .send(tungstenite::Message::Text(handshake.into()))
            .await
            .map_err(|error| error.to_string())?;
        let shared = Arc::new(RelayShared::default());
        let (to_core, mut from_screen) = mpsc::channel(SCREEN_TO_CORE);
        let task = tokio::spawn({
            let shared = Arc::clone(&shared);
            async move {
                let (mut sink, mut stream) = upstream.split();
                let read = async {
                    loop {
                        match stream.next().await {
                            Some(Ok(
                                frame @ (tungstenite::Message::Text(_)
                                | tungstenite::Message::Binary(_)),
                            )) => {
                                if !lock(&shared.backlog).push(frame) {
                                    // Logged now: the screen may not read for a while.
                                    herdr_core::diagnostic!(json!({
                                        "component": "node_daemon",
                                        "kind": "screen.fell_behind",
                                        "connection": connection,
                                        "cap": SCREEN_BACKLOG_BYTES,
                                    }));
                                    shared.ready.notify_one();
                                    return;
                                }
                                shared.ready.notify_one();
                            }
                            Some(Ok(tungstenite::Message::Close(frame))) => {
                                let refusal = frame
                                    .filter(|frame| (4001..=4004).contains(&u16::from(frame.code)))
                                    .map(|frame| CloseFrame {
                                        code: frame.code.into(),
                                        reason: frame.reason.as_str().into(),
                                    });
                                lock(&shared.backlog).end(RelayEnd::Closed(refusal));
                                return;
                            }
                            None | Some(Err(_)) => return,
                            Some(Ok(_)) => {}
                        }
                    }
                };
                let write = async {
                    while let Some(frame) = from_screen.recv().await {
                        if sink.send(frame).await.is_err() {
                            return;
                        }
                    }
                    let _ = sink.close().await;
                };
                tokio::select! {
                    () = read => {}
                    () = write => {}
                }
                lock(&shared.backlog).end(RelayEnd::Closed(None));
                shared.ready.notify_one();
            }
        });
        Ok(Self {
            shared,
            to_core,
            task,
        })
    }

    /// Sends a screen's frame up, waiting while the relay is full.
    async fn up(&self, frame: tungstenite::Message) -> Result<(), ()> {
        self.to_core.send(frame).await.map_err(|_| ())
    }
}

impl Drop for ScreenRelay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A screen's handshake as its core is sent it: without this daemon's
/// screen token, which the core does not read (the relay grant admits the
/// screen) and has no use for.
fn forwarded_handshake(handshake: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(handshake) else {
        return handshake.to_owned();
    };
    if let Some(token) = value.get_mut("token") {
        *token = Value::String(String::new());
    }
    value.to_string()
}

/// How a screen's session ended, and what it is told.
#[derive(Debug)]
enum ScreenEnd {
    /// The screen left.
    Left,
    /// The link to the core ended: the screen reconnects to the held node.
    LinkLost,
    /// The screen left more than [`SCREEN_BACKLOG_BYTES`] of the core's
    /// frames untaken.
    FellBehind,
    /// The core refused the screen, with its own close (4001 to 4004).
    Refused(CloseFrame),
}

impl ScreenEnd {
    fn reason(&self) -> &str {
        match self {
            Self::Left => "screen_closed",
            Self::LinkLost => "link_ended",
            Self::FellBehind => "fell_behind",
            Self::Refused(frame) => frame.reason.as_str(),
        }
    }

    /// The close the screen is sent; none for a screen that left.
    fn close(self) -> Option<CloseFrame> {
        match self {
            Self::Left => None,
            Self::LinkLost => Some(CloseFrame {
                code: 1012,
                reason: "core_link_lost".into(),
            }),
            Self::FellBehind => Some(CloseFrame {
                code: 1013,
                reason: "screen_fell_behind".into(),
            }),
            Self::Refused(frame) => Some(frame),
        }
    }
}

/// Reads the screen and drops what it sends until it leaves.
async fn drop_until_closed(socket: &mut WebSocket, state: &NodeState) {
    loop {
        match socket.recv().await {
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
            Some(Ok(_)) => {
                state.held_frames.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameStart {
    Snapshot,
    Delta,
}

/// Whether a core frame is a whole snapshot or a delta, read from its
/// `type` alone.
fn frame_kind(text: &str) -> Option<FrameStart> {
    let head = text.get(..text.len().min(32))?;
    if head.starts_with(r#"{"type":"snapshot""#) {
        Some(FrameStart::Snapshot)
    } else if head.starts_with(r#"{"type":"delta""#) {
        Some(FrameStart::Delta)
    } else {
        None
    }
}

/// Where a screen's terminals start in this daemon's hub, by the first
/// core frame it got, as a core's daemon decides it.
fn resume(first: FrameStart, handshake: &Handshake) -> Resume {
    match (
        first,
        handshake.have_terminal_sequence,
        handshake.have_terminal_epoch.clone(),
    ) {
        (FrameStart::Snapshot, _, _) => Resume::Fresh,
        (FrameStart::Delta, Some(cursor), Some(epoch)) => Resume::After { epoch, cursor },
        (FrameStart::Delta, _, _) => Resume::Redraw,
    }
}

/// A screen's read of a file of this machine, under a checkout the core
/// opened here (PRD core-host-node-remote-core B4, D-05): the event and the
/// file, opened under its root, or the refusal. `None` for any other event,
/// and for a path under no root the core opened here, which the core
/// answers as it answers any device's.
fn own_file(
    state: &NodeState,
    link: &LiveLink,
    text: &str,
) -> Option<(Value, crate::server::OpenedFile)> {
    if !text.contains(r#""kind":"file_bytes""#) {
        return None;
    }
    let event: Value = serde_json::from_str(text).ok()?;
    let device = event
        .pointer("/payload/device_id")
        .and_then(Value::as_str)?;
    if event.get("kind").and_then(Value::as_str) != Some("file_bytes")
        || state.boundary.node() != device
    {
        return None;
    }
    state.boundary.set_roots(
        link.roots
            .roots()
            .into_iter()
            .map(|root| Root {
                workspace_id: String::new(),
                checkout_id: String::new(),
                path: PathBuf::from(root),
            })
            .collect(),
    );
    let path = event.pointer("/payload/path").and_then(Value::as_str)?;
    match state.boundary.open_file(path) {
        Err(Refusal::OutsideCheckout) => None,
        opened => Some((event, opened)),
    }
}

/// How often a screen tells the core it still types into one pane.
const INPUT_NOTICE_EVERY: Duration = Duration::from_secs(1);
/// The panes a screen's notices are remembered for; past it they start over.
const INPUT_NOTICE_PANES: usize = 64;

/// The panes one screen told its core it typed into, and when: the core
/// sizes a pane at the grid of the screen that last sent it input
/// (`pane_sizes`), and never sees the keys this daemon takes. A screen that
/// keeps typing says so once a second, so another screen's input in
/// between is overruled within that.
#[derive(Default)]
struct InputNotices {
    told: HashMap<String, Instant>,
}

impl InputNotices {
    /// The notice to send for a key into `pane` at `now`, if one is due.
    fn notice(&mut self, pane: &str, now: Instant) -> Option<String> {
        if self
            .told
            .get(pane)
            .is_some_and(|told| now.duration_since(*told) < INPUT_NOTICE_EVERY)
        {
            return None;
        }
        if self.told.len() >= INPUT_NOTICE_PANES && !self.told.contains_key(pane) {
            self.told.clear();
        }
        self.told.insert(pane.to_owned(), now);
        Some(
            json!({
                "schema_version": SCHEMA_VERSION,
                "kind": "terminal_input",
                "payload": {"pane_id": pane},
            })
            .to_string(),
        )
    }
}

/// A screen's key, taken here; `None` for every other event, which goes to
/// the core. A key into a pane answers that pane. A view goes to the core
/// too, which decides the grid every screen's view of a pane is drawn at
/// (`pane_sizes`) and sends it to the pane's node.
fn take_terminal_event(
    state: &NodeState,
    connection: u64,
    text: &str,
) -> Option<Result<Option<String>, String>> {
    // A key names its kind first; anything else is not parsed.
    if !text.contains(r#""kind":"key""#) {
        return None;
    }
    let event: Value = serde_json::from_str(text).ok()?;
    match event.get("kind").and_then(Value::as_str) {
        Some("key") => Some(terminal_key(&event).and_then(|(target, bytes)| {
            let typed = match &target {
                KeyTarget::Pane(pane) => Some(pane.clone()),
                KeyTarget::Request(_) => None,
            };
            state
                .terminals
                .key(connection, target, bytes, crate::server::unix_ms_now())?;
            Ok(typed)
        })),
        _ => None,
    }
}

/// Keeps one terminals relay to the core for each live link: the core's
/// panes' output into this daemon's hub, and screens' keys and
/// redraws for those panes up to the core.
async fn keep_terminals_relay(
    mut live: watch::Receiver<Option<Arc<LiveLink>>>,
    hub: Arc<TerminalHub>,
    terminals: Arc<ScreenTerminals>,
) {
    loop {
        let link = live.borrow_and_update().clone();
        let Some(link) = link else {
            if live.changed().await.is_err() {
                return;
            }
            continue;
        };
        let ended = terminals_relay(&link, &mut live, &hub, &terminals).await;
        herdr_core::diagnostic!(json!({
            "component": "node_daemon",
            "kind": "terminals.relay_ended",
            "generation": link.generation,
            "reason": ended,
        }));
        let still = live
            .borrow()
            .as_ref()
            .is_some_and(|now| now.generation == link.generation);
        if still {
            tokio::time::sleep(RELAY_RETRY).await;
            lock(&terminals.held).expire(Instant::now(), &terminals.dropped);
        } else {
            // Lines typed for a link that ended are never delivered later.
            lock(&terminals.held).clear(&terminals.dropped);
        }
    }
}

async fn terminals_relay(
    link: &LiveLink,
    live: &mut watch::Receiver<Option<Arc<LiveLink>>>,
    hub: &TerminalHub,
    terminals: &Arc<ScreenTerminals>,
) -> String {
    let upstream = match open_relay(link, "terminals").await {
        Ok(upstream) => upstream,
        Err(message) => return format!("open_failed: {message}"),
    };
    let (mut sink, mut from_core) = upstream.split();
    // Written on a task of its own: a write the core is slow to take never
    // stops this relay reading the core's output, which would fill the SSH
    // channel and stall every other one on the connection.
    let mut writer = {
        let terminals = Arc::clone(terminals);
        tokio::spawn(async move {
            loop {
                let next = lock(&terminals.held).next(Instant::now(), &terminals.dropped);
                match next {
                    Some(line) => {
                        if sink
                            .send(tungstenite::Message::Text(line.into()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    None => terminals.ready.notified().await,
                }
            }
        })
    };
    let _abort = AbortOnDrop(writer.abort_handle());
    loop {
        tokio::select! {
            changed = live.changed() => {
                let same = changed.is_ok()
                    && live
                        .borrow()
                        .as_ref()
                        .is_some_and(|now| now.generation == link.generation);
                if !same {
                    return "link_ended".to_owned();
                }
            }
            _ = &mut writer => return "core_closed".to_owned(),
            from_core = from_core.next() => match from_core {
                Some(Ok(tungstenite::Message::Text(text))) => {
                    ingest(hub, &terminals.own_prefix, &text);
                }
                Some(Ok(tungstenite::Message::Close(_))) | None | Some(Err(_)) => {
                    return "core_closed".to_owned();
                }
                Some(Ok(_)) => {}
            },
        }
    }
}

/// Ends a task when its owner ends.
struct AbortOnDrop(tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The core's panes' output, one terminal line each, into the hub.
fn ingest(hub: &TerminalHub, own_prefix: &str, text: &str) {
    for line in text.lines() {
        let output = match serde_json::from_str::<TerminalLine<TerminalUp>>(line) {
            Ok(TerminalLine {
                terminal: TerminalUp::Output(output),
            }) => output,
            // The pane is gone on the core: nothing of it is kept here.
            Ok(TerminalLine {
                terminal: TerminalUp::Forget { pane },
            }) => {
                if !pane.starts_with(own_prefix) {
                    hub.forget(&pane);
                }
                continue;
            }
            _ => continue,
        };
        // The core sends no pane of this machine; one it did is not drawn
        // over this machine's own.
        if output.pane.starts_with(own_prefix) || output.pane.len() > MAX_PANE_ID_BYTES {
            continue;
        }
        if let Ok(bytes) = decode_base64(&output.data) {
            hub.output(&output.pane, &bytes, output.full);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_that_falls_behind_drops_its_backlog_past_the_cap() {
        let mut backlog = Backlog::default();
        let whole = tungstenite::Message::Binary(vec![0_u8; SCREEN_BACKLOG_BYTES + 64].into());
        assert!(backlog.push(whole), "one frame is held however large");
        assert!(backlog.pop().is_some());
        let frame = || tungstenite::Message::Binary(vec![0_u8; 1024 * 1024].into());
        for _ in 0..4 {
            assert!(backlog.push(frame()));
        }
        assert_eq!(backlog.end, None);
        assert!(backlog.pop().is_some(), "a frame taken makes room");
        assert!(backlog.push(frame()));
        assert!(!backlog.push(tungstenite::Message::Text("x".into())));
        assert_eq!(backlog.end, Some(RelayEnd::Overflowed));
        assert!(backlog.pop().is_none(), "what the screen missed is dropped");
        assert!(
            !backlog.push(tungstenite::Message::Text("y".into())),
            "nothing is held after the end"
        );
        backlog.end(RelayEnd::Closed(None));
        assert_eq!(
            backlog.end,
            Some(RelayEnd::Overflowed),
            "the first end stands"
        );
    }

    /// Keys for the core's panes typed while the terminals relay is away
    /// wait in order; past the byte bound the screen is refused at once,
    /// and a key that waited too long is dropped and its screen told (R5).
    #[test]
    fn keys_for_the_core_s_panes_wait_for_the_relay_in_order_and_none_is_lost_unsaid() {
        let terminals =
            ScreenTerminals::new("remote:screen:pane:".to_owned(), watch::channel(None).1);
        let mut dropped = terminals.dropped.subscribe();
        let key = |text: &str| KeyTarget::Pane(format!("core-pane-{text}"));
        terminals.key(1, key("a"), b"a".to_vec(), 1).unwrap();
        terminals.key(2, key("b"), b"b".to_vec(), 2).unwrap();
        let now = Instant::now();
        let mut held = lock(&terminals.held);
        let first = held.next(now, &terminals.dropped).expect("the first key");
        assert!(first.contains("core-pane-a"), "{first}");
        // The second waited too long: dropped, and its screen told.
        let late = now + HELD_FOR + Duration::from_millis(1);
        assert_eq!(held.next(late, &terminals.dropped), None);
        assert_eq!(dropped.try_recv().unwrap(), 2);
        drop(held);

        // A paste larger than the bound goes alone; behind it, a key that
        // would cross the bound is refused.
        let paste = vec![b'x'; HELD_BYTES * 2];
        terminals.key(3, key("c"), paste, 3).unwrap();
        let refused = terminals.key(3, key("d"), b"d".to_vec(), 4).unwrap_err();
        assert!(refused.contains("not sent"), "{refused}");
        // This machine's own panes never wait for the relay.
        terminals
            .key(
                3,
                KeyTarget::Pane("remote:screen:pane:w:p".to_owned()),
                b"x".to_vec(),
                5,
            )
            .expect("an own pane's key");
    }

    #[test]
    fn a_screen_s_handshake_reaches_its_core_without_its_token() {
        let forwarded: Value = serde_json::from_str(&forwarded_handshake(
            r#"{"token":"secret","schema_version":2,"have_revision":7}"#,
        ))
        .unwrap();
        assert_eq!(
            forwarded,
            json!({"token":"","schema_version":2,"have_revision":7})
        );
    }

    #[test]
    fn a_screen_tells_its_core_it_types_into_a_pane_once_a_second() {
        let mut told = InputNotices::default();
        let start = Instant::now();
        let notice = told.notice("p", start).expect("the first key is told");
        let notice: Value = serde_json::from_str(&notice).unwrap();
        assert_eq!(notice["kind"], "terminal_input");
        assert_eq!(notice["payload"]["pane_id"], "p");
        assert!(
            told.notice("p", start + Duration::from_millis(999))
                .is_none()
        );
        assert!(told.notice("q", start).is_some(), "each pane on its own");
        assert!(told.notice("p", start + INPUT_NOTICE_EVERY).is_some());
        // Past the cap the panes start over rather than grow.
        for pane in 0..INPUT_NOTICE_PANES * 2 {
            told.notice(&pane.to_string(), start);
        }
        assert!(told.told.len() <= INPUT_NOTICE_PANES);
    }
}
