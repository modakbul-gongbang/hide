//! One phone's connection on `/ws` (PRD D-04, D-13, D-16, D-17).
//!
//! The first frame names it: `{client_kind: "phone", token}` for a paired
//! phone, `{client_kind: "phone", pair, name}` to pair with the live code.
//! After that the phone may only: open one agent's detail, ask for more of
//! its rows, close it, send that pane a reply or one key, and register or
//! report its push subscription. Anything else is refused and recorded.
//! It never receives a file, a path, a setting or the core's snapshot.
//!
//! Every frame to the phone goes through `encode`, the one place a relay
//! transport would wrap in end-to-end encryption (PRD D-01).

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::pane::{self, Input, PaneError};
use super::projection::{AgentKey, Projection};
use super::store::{Notifications, PushSubscription};
use super::{InputState, Mobile, PhoneMeta, Reservation};

/// How often an open detail reads its pane again.
const DETAIL_INTERVAL: Duration = Duration::from_secs(1);
/// The largest frame a phone may send.
const MAX_FRAME: usize = 16 * 1024;
/// The close code for a refused phone: the PWA shows why from the frame before it.
const CLOSE_REFUSED: u16 = 4001;
/// How often a live phone is pinged; the browser answers on its own.
const PING_INTERVAL: Duration = Duration::from_secs(20);
/// A phone that sent nothing, not even a pong, for this long is gone: its
/// socket through tailscaled can stay half-open for minutes, holding a
/// client slot and a "viewing" that would hold back its push (B33).
const SILENT_LIMIT: Duration = Duration::from_secs(45);

/// The one function every frame to a phone passes through.
pub fn encode(frame: &Value) -> Message {
    Message::Text(frame.to_string().into())
}

fn refusal_logged(kind: &str, phone_id: Option<&str>, reason: &str) {
    herdr_core::diagnostic!(json!({
        "component": "mobile_phone", "kind": kind, "phone_id": phone_id, "reason": reason,
    }));
}

async fn refuse(socket: &mut WebSocket, reason: &str) {
    let _ = socket
        .send(encode(&json!({"type": "refused", "reason": reason})))
        .await;
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: CLOSE_REFUSED,
            reason: reason.to_owned().into(),
        })))
        .await;
}

/// Whether a first frame is a phone's.
pub fn is_phone(first: &Value) -> bool {
    first.get("client_kind").and_then(Value::as_str) == Some("phone")
}

fn agents_frame(projection: &Projection) -> Value {
    json!({"type": "agents", "groups": projection.groups})
}

fn meta_frame(meta: &PhoneMeta) -> Value {
    json!({
        "type": "meta",
        "push_mode": meta.push_mode.as_str(),
        // Other phones on a live connection, for "폰 n대 더 연결됨".
        "other_phones": meta.live_phones.saturating_sub(1),
    })
}

struct Detail {
    lines: tokio::sync::watch::Sender<u32>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Detail {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Reads the open pane once a second and sends its rows when their text or
/// the asked line count changed. Herdr's read `revision` counts pane state,
/// not output, so it cannot say that new rows arrived.
fn spawn_detail(
    mobile: Arc<Mobile>,
    key: AgentKey,
    frames: mpsc::Sender<Value>,
    mut lines: tokio::sync::watch::Receiver<u32>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last: Option<(String, u32)> = None;
        let mut last_error: Option<&'static str> = None;
        loop {
            let asked = *lines.borrow_and_update();
            let pane_id = key.herdr_pane_id().to_owned();
            let device_id = key.device_id.clone();
            let reader = Arc::clone(&mobile);
            // Resolving a device's connection waits on the core owner thread,
            // so it runs on the blocking pool with the read itself.
            let read = tokio::task::spawn_blocking(move || {
                reader
                    .herdr_api(&device_id)
                    .and_then(|connector| pane::read(&connector, &pane_id, asked))
            })
            .await
            .unwrap_or_else(|error| Err(PaneError::Unavailable(error.to_string())));
            let frame = match read {
                Ok(rows) => {
                    last_error = None;
                    if last
                        .as_ref()
                        .is_some_and(|(text, lines)| *lines == asked && *text == rows.text)
                    {
                        None
                    } else {
                        let frame = json!({
                            "type": "rows", "device_id": key.device_id, "pane_id": key.pane_id,
                            "state": "ok", "text": rows.text, "lines": asked,
                            // Herdr says whether it cut older rows off this read.
                            "more": asked < pane::MAX_LINES && rows.truncated,
                        });
                        last = Some((rows.text, asked));
                        Some(frame)
                    }
                }
                Err(error) => {
                    last = None;
                    if last_error == Some(error.reason()) {
                        None
                    } else {
                        last_error = Some(error.reason());
                        if let PaneError::Unavailable(message) = &error {
                            herdr_core::diagnostic!(json!({
                                "component": "mobile_phone", "kind": "pane.read_failed", "message": message,
                            }));
                        }
                        Some(json!({
                            "type": "rows", "device_id": key.device_id, "pane_id": key.pane_id,
                            "state": error.reason(),
                        }))
                    }
                }
            };
            if let Some(frame) = frame
                && frames.send(frame).await.is_err()
            {
                return;
            }
            tokio::select! {
                _ = tokio::time::sleep(DETAIL_INTERVAL) => {}
                changed = lines.changed() => if changed.is_err() { return },
            }
        }
    })
}

fn key_of(message: &Value) -> Option<AgentKey> {
    let device_id = message.get("device_id").and_then(Value::as_str)?;
    let pane_id = message.get("pane_id").and_then(Value::as_str)?;
    (!device_id.is_empty() && device_id.len() <= 256 && !pane_id.is_empty() && pane_id.len() <= 512)
        .then(|| AgentKey {
            device_id: device_id.to_owned(),
            pane_id: pane_id.to_owned(),
        })
}

fn request_id_of(message: &Value) -> Option<&str> {
    message
        .get("request_id")
        .and_then(Value::as_str)
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

/// Serves a phone after the daemon counted it toward `MAX_CLIENTS`.
pub async fn serve(
    mut socket: WebSocket,
    first: Value,
    mobile: Arc<Mobile>,
    connection: u64,
    user_agent: String,
) {
    let (phone, paired) = if let Some(code) = first.get("pair").and_then(Value::as_str) {
        let hint = first
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| name.len() <= 200)
            .map(str::to_owned)
            .unwrap_or(user_agent);
        match mobile.pair(code, &hint) {
            Ok((phone, credential)) => {
                let paired = json!({
                    "type": "paired", "credential": credential, "phone_id": phone.id, "name": phone.name,
                });
                (phone, Some(paired))
            }
            Err(refusal) => {
                refuse(&mut socket, refusal.reason()).await;
                return;
            }
        }
    } else if let Some(token) = first.get("token").and_then(Value::as_str) {
        match mobile.authenticate(token) {
            Ok(phone) => (phone, None),
            Err(reason) => {
                refusal_logged("phone.refused", None, reason);
                refuse(&mut socket, reason).await;
                return;
            }
        }
    } else {
        refusal_logged("phone.refused", None, "no_credential");
        refuse(&mut socket, "revoked").await;
        return;
    };
    // Registered before anything is sent, then checked again: a revoke or a
    // switch-off that lands in between closes this socket like any other.
    let close = mobile.register(connection, &phone.id);
    if let Err(reason) = mobile.still_admitted(&phone.id) {
        refuse(&mut socket, reason).await;
        mobile.unregister(connection);
        return;
    }
    if let Some(paired) = paired
        && socket.send(encode(&paired)).await.is_err()
    {
        mobile.unregister(connection);
        return;
    }
    run(&mut socket, &mobile, connection, &phone.id, close).await;
    mobile.unregister(connection);
}

async fn run(
    socket: &mut WebSocket,
    mobile: &Arc<Mobile>,
    connection: u64,
    phone_id: &str,
    close: Arc<tokio::sync::Notify>,
) {
    let mut projection = mobile.subscribe_projection();
    let mut meta = mobile.subscribe_meta();
    let (frames_tx, mut frames) = mpsc::channel::<Value>(8);
    let mut detail: Option<Detail> = None;
    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut heard = tokio::time::Instant::now();
    let record = mobile.phone(phone_id);
    let hello = json!({
        "type": "hello",
        "mac_name": mobile.mac_name(),
        "phone_id": phone_id,
        "name": record.as_ref().map(|phone| phone.name.clone()),
        "vapid_public_key": mobile.vapid_public_key(),
        "notifications": match record.as_ref().map(|phone| (phone.notifications, phone.push.is_some())) {
            Some((_, true)) => "on",
            Some((Notifications::Off, false)) => "off",
            _ => "unasked",
        },
    });
    let first = [
        hello,
        meta_frame(&meta.borrow_and_update()),
        agents_frame(&projection.borrow_and_update()),
    ];
    for frame in &first {
        if socket.send(encode(frame)).await.is_err() {
            return;
        }
    }
    loop {
        tokio::select! {
            _ = close.notified() => {
                let reason = if mobile.phone(phone_id).is_some() { "mobile_off" } else { "revoked" };
                refuse(socket, reason).await;
                return;
            }
            changed = projection.changed() => {
                if changed.is_err() { return; }
                let frame = agents_frame(&projection.borrow_and_update());
                if socket.send(encode(&frame)).await.is_err() { return; }
            }
            changed = meta.changed() => {
                if changed.is_err() { return; }
                let frame = meta_frame(&meta.borrow_and_update());
                if socket.send(encode(&frame)).await.is_err() { return; }
            }
            Some(frame) = frames.recv() => {
                if socket.send(encode(&frame)).await.is_err() { return; }
            }
            _ = ping.tick() => {
                if heard.elapsed() >= SILENT_LIMIT {
                    herdr_core::diagnostic!(json!({
                        "component": "mobile_phone", "kind": "phone.silent", "phone_id": phone_id,
                    }));
                    return;
                }
                if socket.send(Message::Ping(Default::default())).await.is_err() { return; }
            }
            incoming = socket.recv() => {
                heard = tokio::time::Instant::now();
                let text = match incoming {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                    Some(Ok(Message::Pong(_) | Message::Ping(_))) => continue,
                    Some(Ok(_)) => {
                        refusal_logged("scope.refused", Some(phone_id), "binary");
                        continue;
                    }
                };
                if text.len() > MAX_FRAME {
                    refusal_logged("scope.refused", Some(phone_id), "too_large");
                    continue;
                }
                let Ok(message) = serde_json::from_str::<Value>(&text) else {
                    refusal_logged("scope.refused", Some(phone_id), "not_json");
                    continue;
                };
                // A clone of the current list: a watch borrow must not be held across an await.
                let current = Arc::clone(&projection.borrow());
                let reply = handle(mobile, connection, phone_id, &message, &mut detail, &frames_tx, &current).await;
                if let Some(reply) = reply
                    && socket.send(encode(&reply)).await.is_err()
                {
                    return;
                }
            }
        }
    }
}

/// One phone message. The phone's whole vocabulary is here.
async fn handle(
    mobile: &Arc<Mobile>,
    connection: u64,
    phone_id: &str,
    message: &Value,
    detail: &mut Option<Detail>,
    frames: &mpsc::Sender<Value>,
    projection: &Projection,
) -> Option<Value> {
    let kind = message.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "open" => {
            let Some(key) = key_of(message).filter(|key| projection.find(key).is_some()) else {
                refusal_logged("scope.refused", Some(phone_id), "not_an_agent");
                return Some(
                    json!({"type": "rows", "device_id": message.get("device_id"), "pane_id": message.get("pane_id"), "state": "gone"}),
                );
            };
            let (lines, receiver) = tokio::sync::watch::channel(pane::FIRST_LINES);
            mobile.set_viewing(connection, Some(key.clone()));
            let task = spawn_detail(Arc::clone(mobile), key, frames.clone(), receiver);
            *detail = Some(Detail { lines, task });
            None
        }
        "more" => {
            if let Some(detail) = detail.as_ref() {
                detail
                    .lines
                    .send_modify(|lines| *lines = (*lines + pane::MORE_LINES).min(pane::MAX_LINES));
            }
            None
        }
        "close" => {
            *detail = None;
            mobile.set_viewing(connection, None);
            None
        }
        "input" => Some(input(mobile, phone_id, message, projection).await),
        "push_subscription" => {
            let subscription = message.get("subscription");
            let field = |name: &str| {
                subscription
                    .and_then(|value| value.pointer(name))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned()
            };
            let push = PushSubscription {
                endpoint: field("/endpoint"),
                p256dh: field("/keys/p256dh"),
                auth: field("/keys/auth"),
            };
            let stored = mobile.set_subscription(phone_id, push);
            Some(
                json!({"type": "push_state", "notifications": if stored { "on" } else { "refused" }}),
            )
        }
        "push_permission" => {
            let notifications = match message.get("state").and_then(Value::as_str) {
                Some("denied") => Notifications::Off,
                Some("default") => Notifications::Unasked,
                _ => {
                    refusal_logged("scope.refused", Some(phone_id), "push_permission");
                    return None;
                }
            };
            mobile.set_notifications(phone_id, notifications);
            None
        }
        other => {
            // A file, a setting, a project or device change, the snapshot:
            // nothing a phone credential may ask for (B17).
            refusal_logged(
                "scope.refused",
                Some(phone_id),
                &format!("type:{}", other.chars().take(64).collect::<String>()),
            );
            Some(
                json!({"type": "refused_request", "request": other.chars().take(64).collect::<String>(), "reason": "not_allowed"}),
            )
        }
    }
}

async fn input(
    mobile: &Arc<Mobile>,
    phone_id: &str,
    message: &Value,
    projection: &Projection,
) -> Value {
    let Some(request_id) = request_id_of(message) else {
        return json!({"type": "input_result", "ok": false, "reason": "invalid_request"});
    };
    let answer = |ok: bool, reason: Option<&str>| json!({"type": "input_result", "request_id": request_id, "ok": ok, "reason": reason});
    let Some(key) = key_of(message).filter(|key| projection.find(key).is_some()) else {
        refusal_logged("scope.refused", Some(phone_id), "input_not_an_agent");
        return answer(false, Some("gone"));
    };
    let text = message.get("text").and_then(Value::as_str);
    let herdr_key = message.get("key").and_then(Value::as_str);
    let problem = match (text, herdr_key) {
        (Some(text), None) => pane::reply_problem(text),
        (None, Some(key)) => pane::herdr_key(key).is_none().then_some("unknown_key"),
        _ => Some("invalid_request"),
    };
    if let Some(problem) = problem {
        return answer(false, Some(problem));
    }
    // One claim per request id, taken before writing: a repeat that arrives
    // while the first is still being written, or after Herdr's answer was
    // lost, is answered from the claim and never written again (B27).
    match mobile.reserve_input(phone_id, request_id) {
        Reservation::Write => {}
        Reservation::Seen(InputState::Written) => return answer(true, None),
        Reservation::Seen(InputState::InFlight) => return answer(false, Some("in_flight")),
        Reservation::Seen(InputState::Uncertain) => return answer(false, Some("uncertain")),
    }
    let pane_id = key.herdr_pane_id().to_owned();
    let device_id = key.device_id.clone();
    let writer = Arc::clone(mobile);
    let text = text.map(str::to_owned);
    let herdr_key = herdr_key.and_then(pane::herdr_key);
    // `attempted` is whether the write reached Herdr at all.
    let (attempted, result) = tokio::task::spawn_blocking(move || {
        let connector = match writer.herdr_api(&device_id) {
            Ok(connector) => connector,
            Err(error) => return (false, Err(error)),
        };
        let input = match (&text, herdr_key) {
            (Some(text), _) => Input::Reply(text),
            (None, Some(key)) => Input::Key(key),
            (None, None) => unreachable!("validated above"),
        };
        (true, pane::send(&connector, &pane_id, input))
    })
    .await
    .unwrap_or_else(|error| (true, Err(PaneError::Unavailable(error.to_string()))));
    // Nothing reached Herdr, or Herdr says the pane is gone: nothing was
    // written and the id may be sent again. Any other failure may have landed.
    let certain =
        !attempted || matches!(result, Err(PaneError::Gone | PaneError::DeviceUnreachable));
    let settled = match &result {
        Ok(()) => Some(InputState::Written),
        Err(_) if certain => None,
        Err(_) => Some(InputState::Uncertain),
    };
    mobile.settle_input(phone_id, request_id, settled);
    match result {
        Ok(()) => {
            herdr_core::diagnostic!(json!({
                "component": "mobile_phone", "kind": "input.sent", "phone_id": phone_id,
                "device_id": key.device_id, "kind_of_input": if message.get("key").is_some() { "key" } else { "reply" },
            }));
            answer(true, None)
        }
        Err(error) => {
            herdr_core::diagnostic!(json!({
                "component": "mobile_phone", "kind": "input.failed", "phone_id": phone_id,
                "reason": error.reason(),
                "message": match &error { PaneError::Unavailable(message) => Some(message.clone()), _ => None },
            }));
            let reason = if certain { error.reason() } else { "uncertain" };
            answer(false, Some(reason))
        }
    }
}
