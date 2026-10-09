//! Browser control on a screen machine whose core runs elsewhere (PRD
//! core-host-node-remote-core D-06, B4, B13, B15). The desktop window on
//! this machine registers its CDP gateway with this daemon, as it would
//! with a core's, and the registration never leaves the machine: the core
//! learns only that a window is here (its owner pid), over the link.
//!
//! Authority stays with the core. It checks a caller's credential, decides
//! the Workspace, area and display, and asks this daemon over the link for
//! the gateway's capability in that scope (`browser_gateway`). For a caller
//! on this machine the answer is the gateway's own loopback URLs, or a
//! one-shot relay ticket on this daemon, whose relay runs the caller's CDP
//! to the gateway here: none of it crosses the link. For a caller on
//! another machine the core reaches the same relay through a link stream.
//! The page actions the gateway asks for (open, close, select) go to the
//! core, which runs them as it runs its own window's.
//!
//! Everything handed out follows the link it was handed out over: when the
//! link ends, its tickets are dropped, its relays end as a lost gateway, and
//! the gateway is asked to revoke every capability it issued.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use tokio::sync::{Notify, Semaphore, watch};

use crate::browser_control::{BrowserControl, Failure, Registration};
use crate::node_daemon::NodeState;
use crate::node_role::LiveLink;
use crate::server::RELAY_GRANT_HEADER;

/// Tickets waiting to be dialed at once.
const MAX_TICKETS: usize = 8;
/// How long a ticket waits to be dialed.
const TICKET_LIFE: Duration = Duration::from_secs(10);
/// Relays this daemon runs at once, as many as a core runs.
const MAX_RELAYS: usize = 4;
/// How long an announce that failed waits before it is sent again.
const ANNOUNCE_RETRY: Duration = Duration::from_secs(2);
/// The largest answer the core sends a page action.
const MAX_ACTION_ANSWER: u64 = 64 * 1024;
const CORE_TIMEOUT: Duration = Duration::from_secs(12);

struct Ticket {
    browser_ws_url: String,
    display_id: String,
    generation: u64,
    issued: Instant,
}

/// This machine's desktop windows and the relays handed out for them.
pub struct NodeBrowser {
    control: BrowserControl,
    tickets: Mutex<HashMap<String, Ticket>>,
    relays: Arc<Semaphore>,
    /// This daemon's own loopback port, where its relay listens.
    port: u16,
    /// A window registered or left: the core is told again.
    changed: Notify,
}

impl NodeBrowser {
    pub fn new(port: u16) -> Arc<Self> {
        Arc::new(Self {
            control: BrowserControl::default(),
            tickets: Mutex::new(HashMap::new()),
            relays: Arc::new(Semaphore::new(MAX_RELAYS)),
            port,
            changed: Notify::new(),
        })
    }

    /// The core's question over the link of `generation`.
    fn capability(&self, scope: &Value, relay: bool, generation: u64) -> Result<Value, String> {
        let failed = |(reason, _): Failure| reason.to_owned();
        let display_id = scope_valid(scope).ok_or("invalid_browser_scope")?;
        let (http, ws) = self.control.gateway_capability(scope).map_err(failed)?;
        if !relay {
            return Ok(json!({"cdp_http_url": http, "browser_ws_url": ws}));
        }
        let display_id = display_id.ok_or("browser_display_missing")?;
        let mut tickets = lock(&self.tickets);
        tickets.retain(|_, ticket| ticket.issued.elapsed() < TICKET_LIFE);
        if tickets.len() >= MAX_TICKETS {
            return Err("browser_relay_limit".to_owned());
        }
        let ticket = crate::state_file::new_token();
        tickets.insert(
            ticket.clone(),
            Ticket {
                browser_ws_url: ws,
                display_id,
                generation,
                issued: Instant::now(),
            },
        );
        Ok(json!({
            "relay_url": format!("ws://127.0.0.1:{}/browser-relay/{ticket}", self.port)
        }))
    }

    /// Takes `ticket` for a dial on the link of `generation`.
    fn take(&self, ticket: &str, generation: Option<u64>) -> Option<Ticket> {
        let ticket = lock(&self.tickets).remove(ticket)?;
        (Some(ticket.generation) == generation && ticket.issued.elapsed() < TICKET_LIFE)
            .then_some(ticket)
    }

    /// Follows the link: tells each core that takes it which windows are
    /// here, again whenever that changes, and revokes everything handed out
    /// over a link once it is gone.
    pub fn spawn_follow(
        self: &Arc<Self>,
        mut live: watch::Receiver<Option<Arc<LiveLink>>>,
    ) -> tokio::task::JoinHandle<()> {
        let browser = Arc::clone(self);
        tokio::spawn(async move {
            let mut current: Option<u64> = None;
            let mut announced = false;
            loop {
                let link = live.borrow_and_update().clone();
                let generation = link.as_ref().map(|link| link.generation);
                if current.is_some() && current != generation {
                    browser.ended().await;
                    announced = false;
                }
                current = generation;
                if let Some(link) = link.filter(|_| !announced) {
                    let owners = browser.control.owners();
                    let sent = tokio::task::spawn_blocking(move || announce(&link, &owners))
                        .await
                        .unwrap_or_else(|_| Err("announce_failed".to_owned()));
                    match sent {
                        Ok(()) => announced = true,
                        Err(reason) => herdr_core::diagnostic!(json!({
                            "component": "node_browser",
                            "kind": "announce.failed",
                            "generation": generation,
                            "reason": reason,
                        })),
                    }
                }
                tokio::select! {
                    changed = live.changed() => {
                        if changed.is_err() {
                            browser.ended().await;
                            return;
                        }
                    }
                    () = browser.changed.notified() => announced = false,
                    () = tokio::time::sleep(ANNOUNCE_RETRY), if !announced && current.is_some() => {}
                }
            }
        })
    }

    /// The link is gone: its tickets go, and the gateway drops every
    /// capability; each relay ends on its own as a lost gateway.
    async fn ended(self: &Arc<Self>) {
        lock(&self.tickets).clear();
        let browser = Arc::clone(self);
        let _ = tokio::task::spawn_blocking(move || browser.control.revoke_all()).await;
    }
}

/// The browser gateway as the link of one generation serves it.
pub struct LinkBrowser {
    pub browser: Arc<NodeBrowser>,
    pub generation: u64,
}

impl hide_host::link_bridge::BrowserGateway for LinkBrowser {
    fn capability(&self, scope: &Value, relay: bool) -> Result<Value, String> {
        self.browser.capability(scope, relay, self.generation)
    }

    fn relay_address(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.browser.port))
    }
}

/// The scope the core decided, in the shape the gateway takes and nothing
/// else; its display, if it names one.
fn scope_valid(scope: &Value) -> Option<Option<String>> {
    let fields = scope.as_object()?;
    let text = |key: &str, cap: usize| {
        fields
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= cap)
    };
    text("workspace", 8 * 1024 + 256)?;
    text("area_id", 256)?;
    let display = match fields.get("display_id") {
        None => None,
        Some(_) => Some(text("display_id", 256)?.to_owned()),
    };
    fields
        .keys()
        .all(|key| matches!(key.as_str(), "workspace" | "area_id" | "display_id"))
        .then_some(display)
}

fn core_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(CORE_TIMEOUT))
        .max_redirects(0)
        .proxy(None)
        .http_status_as_error(false)
        .build()
        .into()
}

/// Tells the core over `link` which windows are here now.
fn announce(link: &LiveLink, owners: &[i32]) -> Result<(), String> {
    let response = core_agent()
        .post(format!(
            "http://127.0.0.1:{}/relay/browser-control",
            link.relay_port
        ))
        .header(RELAY_GRANT_HEADER, &link.accepted.relay_token)
        .header("Content-Type", "application/json")
        .send(json!({ "owners": owners }).to_string().as_bytes())
        .map_err(|error| error.to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("the core answered {}", response.status()))
    }
}

fn failure(status: StatusCode, (reason, next_action): Failure) -> Response {
    herdr_core::diagnostic!(json!({
        "component": "node_browser",
        "kind": "request.refused",
        "reason": reason,
    }));
    (
        status,
        axum::Json(json!({"ok": false, "reason": reason, "next_action": next_action})),
    )
        .into_response()
}

/// The desktop window on this machine registers its gateway here.
pub async fn register(
    State(state): State<NodeState>,
    headers: HeaderMap,
    axum::Json(request): axum::Json<Registration>,
) -> Response {
    if !crate::server::bearer_matches(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.browser.control.register(request) {
        Ok(()) => {
            state.browser.changed.notify_one();
            StatusCode::NO_CONTENT.into_response()
        }
        Err(refused) => failure(StatusCode::CONFLICT, refused),
    }
}

pub async fn release(
    State(state): State<NodeState>,
    headers: HeaderMap,
    axum::Json(request): axum::Json<Registration>,
) -> Response {
    if !crate::server::bearer_matches(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.browser.control.release(&request) {
        Ok(()) => {
            state.browser.changed.notify_one();
            StatusCode::NO_CONTENT.into_response()
        }
        Err(refused) => failure(StatusCode::CONFLICT, refused),
    }
}

/// A page action the window's gateway asks for: the core runs it, as the
/// window registered here.
pub async fn action(
    State(state): State<NodeState>,
    headers: HeaderMap,
    axum::Json(request): axum::Json<crate::browser_control::BrowserAction>,
) -> Response {
    if !crate::server::bearer_matches(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !request.valid() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if let Err(refused) = state.browser.control.registered(request.owner_pid) {
        return failure(StatusCode::CONFLICT, refused);
    }
    let Some(link) = state.live.borrow().clone() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            (
                "browser_control_unavailable",
                "Wait for this machine to reach its core and retry",
            ),
        );
    };
    let forwarded = tokio::task::spawn_blocking(move || {
        let body = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        let mut response = core_agent()
            .post(format!(
                "http://127.0.0.1:{}/relay/browser-control/action",
                link.relay_port
            ))
            .header(RELAY_GRANT_HEADER, &link.accepted.relay_token)
            .header("Content-Type", "application/json")
            .send(&body[..])
            .map_err(|error| error.to_string())?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_ACTION_ANSWER)
            .read_to_vec()
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((status, body))
    })
    .await;
    match forwarded {
        Ok(Ok((status, body))) => (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response(),
        // The core may have run it: the gateway reads this as uncertain.
        _ => StatusCode::BAD_GATEWAY.into_response(),
    }
}

/// A caller dials the relay a ticket names: the core handed it out over
/// the live link, for this caller only, moments ago. The gateway side opens
/// first, so its refusal (its own client cap) reaches the caller as a
/// status rather than a dropped socket.
pub async fn relay(
    upgrade: WebSocketUpgrade,
    Path(ticket): Path<String>,
    headers: HeaderMap,
    State(state): State<NodeState>,
) -> Response {
    // A process on this machine, never a page or `tailscale serve`.
    if headers.contains_key(axum::http::header::ORIGIN) || crate::server::via_tailnet(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let generation = state.live.borrow().as_ref().map(|link| link.generation);
    let Some(ticket) = state.browser.take(&ticket, generation) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(slot) = Arc::clone(&state.browser.relays).try_acquire_owned() else {
        herdr_core::diagnostic!(json!({
            "component": "node_browser",
            "kind": "relay.cap_reached",
            "cap": MAX_RELAYS,
        }));
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let gateway = match crate::browser_relay::connect(&ticket.browser_ws_url).await {
        Ok(gateway) => gateway,
        Err((reason, _)) => {
            return if reason == "browser_control_busy" {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::BAD_GATEWAY
            }
            .into_response();
        }
    };
    let live = state.live.clone();
    let generation = ticket.generation;
    upgrade.on_upgrade(move |mut socket| async move {
        let _slot = slot;
        crate::browser_relay::pump(
            &mut socket,
            gateway,
            &ticket.display_id,
            crate::browser_relay::Way::Node,
            link_gone(live, generation),
        )
        .await;
    })
}

/// Resolves once the link of `generation` is gone.
async fn link_gone(mut live: watch::Receiver<Option<Arc<LiveLink>>>, generation: u64) {
    loop {
        if live
            .borrow_and_update()
            .as_ref()
            .map(|link| link.generation)
            != Some(generation)
        {
            return;
        }
        if live.changed().await.is_err() {
            return;
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
    fn a_scope_is_taken_only_in_the_gateway_s_shape() {
        let scope = json!({"workspace": "local\u{0}/c", "area_id": "a", "display_id": "d"});
        assert_eq!(scope_valid(&scope), Some(Some("d".to_owned())));
        assert_eq!(
            scope_valid(&json!({"workspace": "local\u{0}/c", "area_id": "a"})),
            Some(None)
        );
        for refused in [
            json!({"workspace": "local\u{0}/c"}),
            json!({"workspace": "local\u{0}/c", "area_id": ""}),
            json!({"workspace": "local\u{0}/c", "area_id": "a", "display_id": 1}),
            json!({"workspace": "local\u{0}/c", "area_id": "a", "endpoint": "x"}),
            json!("local"),
        ] {
            assert_eq!(scope_valid(&refused), None, "{refused}");
        }
    }

    #[test]
    fn a_ticket_is_taken_once_and_only_on_the_link_it_was_handed_out_over() {
        let browser = NodeBrowser::new(4242);
        let ticket = |browser: &NodeBrowser, generation| {
            lock(&browser.tickets).insert(
                format!("t{generation}"),
                Ticket {
                    browser_ws_url: "ws://127.0.0.1:1/cdp".to_owned(),
                    display_id: "d".to_owned(),
                    generation,
                    issued: Instant::now(),
                },
            );
            format!("t{generation}")
        };
        let first = ticket(&browser, 1);
        assert!(browser.take(&first, Some(1)).is_some());
        assert!(browser.take(&first, Some(1)).is_none(), "one dial only");
        let second = ticket(&browser, 1);
        assert!(browser.take(&second, Some(2)).is_none(), "another link");
        assert!(
            browser.take(&second, Some(1)).is_none(),
            "gone after a refusal"
        );
        let third = ticket(&browser, 1);
        assert!(browser.take(&third, None).is_none(), "no link");
    }

    #[test]
    fn a_relay_without_a_registered_window_is_refused_with_its_reason() {
        let browser = NodeBrowser::new(4242);
        let scope = json!({"workspace": "local\u{0}/c", "area_id": "a", "display_id": "d"});
        assert_eq!(
            browser.capability(&scope, true, 1).unwrap_err(),
            "browser_control_unavailable"
        );
        assert_eq!(
            browser
                .capability(&json!({"workspace": "x"}), true, 1)
                .unwrap_err(),
            "invalid_browser_scope"
        );
    }
}
