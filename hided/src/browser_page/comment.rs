//! The operator's Comment mode on a browser View (PRD view-opener-feedback
//! D-07, B9-B11): a page is a native view the shell cannot draw over, so
//! while Comment is on, hided holds the View's debugger lease, draws
//! `comment.js` in an isolated world of the page, and turns what the
//! operator does there into the core's `view_comment` events.
//!
//! A session is the View's in its checkout (display ids repeat across
//! checkouts), belongs to the screen that turned it on and ends with it,
//! with Comment off or Esc, and when the page's lease ends. The page's scripts
//! cannot reach the binding the overlay reports through (it exists only in
//! the `hide-comment` world), and the overlay reads only trusted input, so a
//! page cannot leave a comment in the operator's name. While a session
//! holds the lease, an agent's `hide browser` command on that page is
//! answered `display_busy` (the PRD's known effect).
//!
//! A comment's screenshot is the page clipped to what it points at, staged
//! on this machine's state folder; only its name reaches the core (D-23).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use super::Failure;
use super::cdp::Cdp;
use super::page::Page;

const COMMENT_JS: &str = include_str!("../../assets/browser/comment.js");
const WORLD: &str = "hide-comment";
const BINDING: &str = "__hideCommentSend";
/// The events a session reads besides a command's.
const EVENTS: &[&str] = &[
    "Runtime.bindingCalled",
    "Page.frameNavigated",
    "Inspector.detached",
];
/// Comment sessions at once: one per View the operator comments on.
const MAX_SESSIONS: usize = 4;
/// What one overlay message may be; a note is at most 4000 characters.
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const CALL: Duration = Duration::from_secs(10);
/// The most pixels a screenshot clip keeps; a larger area is scaled down
/// so the capture stays well under the gateway's message cap.
const MAX_SHOT_PIXELS: f64 = 2_000_000.0;

/// What a session needs of the daemon.
#[derive(Clone)]
pub struct Context {
    pub core: Arc<crate::core::CoreHandle>,
    pub attachments: Arc<crate::attachments::Attachments>,
    pub control: Arc<crate::browser_control::BrowserControl>,
}

/// The page's pins again, while the mode is on.
struct Pins {
    pins: Value,
    full: bool,
    strings: Value,
}

struct Session {
    connection: u64,
    generation: u64,
    pins: mpsc::Sender<Pins>,
    /// Ending the session never waits on the pins' queue: taking this ends
    /// it, so a full queue cannot keep a page held (`None` once it is ending).
    off: Option<oneshot::Sender<()>>,
}

/// A View in its checkout: device, checkout path and display id.
type SessionKey = (String, String, String);

#[derive(Default)]
pub struct CommentSessions {
    sessions: Mutex<HashMap<SessionKey, Session>>,
    generation: std::sync::atomic::AtomicU64,
}

/// The screen's request, `browser_comment`.
struct Request {
    device_id: String,
    path: String,
    area_id: String,
    display_id: String,
    on: bool,
    /// Only the pins again: never starts a session.
    update: bool,
    pins: Value,
    full: bool,
    strings: Value,
}

fn request(event: &Value) -> Result<Request, String> {
    let payload = event
        .get("payload")
        .ok_or("browser_comment has no payload")?;
    let text = |pointer: &str| {
        payload
            .pointer(pointer)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 4096)
            .map(str::to_owned)
            .ok_or_else(|| format!("browser_comment{pointer} is missing"))
    };
    Ok(Request {
        device_id: text("/workspace/device_id")?,
        path: text("/workspace/path")?,
        area_id: text("/area_id")?,
        display_id: text("/display_id")?,
        on: payload["on"].as_bool().unwrap_or(false),
        update: payload["update"].as_bool().unwrap_or(false),
        pins: payload.get("pins").cloned().unwrap_or(json!([])),
        full: payload["full"].as_bool().unwrap_or(false),
        strings: payload.get("strings").cloned().unwrap_or(json!({})),
    })
}

fn frame((device_id, path, display_id): &SessionKey, on: bool, reason: Option<&str>) -> String {
    json!({"type": "browser_comment", "payload": {
        "workspace": {"device_id": device_id, "path": path},
        "display_id": display_id, "on": on, "reason": reason,
    }})
    .to_string()
}

impl Request {
    fn key(&self) -> SessionKey {
        (
            self.device_id.clone(),
            self.path.clone(),
            self.display_id.clone(),
        )
    }
}

impl CommentSessions {
    /// A screen turns Comment on or off for a View, or sends the page's
    /// pins again.
    pub fn handle(
        self: &Arc<Self>,
        context: Context,
        connection: u64,
        event: &Value,
        frames: mpsc::Sender<String>,
    ) -> Result<(), String> {
        let request = request(event)?;
        let key = request.key();
        let mut sessions = self.sessions.lock().map_err(|_| "comment sessions")?;
        if let Some(session) = sessions.get_mut(&key) {
            if !request.on {
                if session.connection == connection {
                    session.off.take();
                }
                return Ok(());
            }
            // Another screen's, or one still letting the page go.
            if session.connection != connection || session.off.is_none() {
                let _ = frames.try_send(frame(&key, false, Some("display_busy")));
                return Ok(());
            }
            // A full queue means the session is still drawing the last
            // pins; the next snapshot sends them again.
            let _ = session.pins.try_send(Pins {
                pins: request.pins,
                full: request.full,
                strings: request.strings,
            });
            return Ok(());
        }
        if !request.on || request.update {
            return Ok(());
        }
        if sessions.len() >= MAX_SESSIONS {
            let _ = frames.try_send(frame(&key, false, Some("comment_limit")));
            return Ok(());
        }
        let (pins, pins_rx) = mpsc::channel(8);
        let (off, off_rx) = oneshot::channel();
        let generation = self
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        sessions.insert(
            key.clone(),
            Session {
                connection,
                generation,
                pins,
                off: Some(off),
            },
        );
        drop(sessions);
        let registry = Arc::clone(self);
        tokio::spawn(async move {
            let display_id = request.display_id.clone();
            let outcome = run(&context, request, pins_rx, off_rx, &frames).await;
            if let Err(failure) = &outcome {
                herdr_core::diagnostic!(json!({
                    "component": "browser_comment",
                    "kind": "session.ended",
                    "display_id": display_id,
                    "reason": failure.reason,
                }));
            }
            if let Ok(mut sessions) = registry.sessions.lock()
                && sessions
                    .get(&key)
                    .is_some_and(|session| session.generation == generation)
            {
                sessions.remove(&key);
            }
            let reason = outcome.err().map(|failure| failure.reason);
            let _ = frames.send(frame(&key, false, reason.as_deref())).await;
        });
        Ok(())
    }

    /// The screen went: its sessions end and their leases with them.
    pub fn release(&self, connection: u64) {
        if let Ok(mut sessions) = self.sessions.lock() {
            for session in sessions
                .values_mut()
                .filter(|session| session.connection == connection)
            {
                session.off.take();
            }
        }
    }
}

/// One session: lease, overlay, and the loop that reads the operator's
/// comments and the screen's pins until either side ends it.
async fn run(
    context: &Context,
    request: Request,
    mut controls: mpsc::Receiver<Pins>,
    mut off: oneshot::Receiver<()>,
    frames: &mpsc::Sender<String>,
) -> Result<(), Failure> {
    let scope = json!({
        "workspace": format!("{}\0{}", request.device_id, request.path),
        "area_id": request.area_id,
        "display_id": request.display_id,
    });
    let control = Arc::clone(&context.control);
    let (_, ws) = tokio::task::spawn_blocking(move || control.gateway_capability(&scope))
        .await
        .map_err(|_| Failure::new("browser_control_unavailable", None))?
        .map_err(|(reason, _)| Failure::new(reason, None))?;
    let socket = crate::browser_relay::connect(&ws)
        .await
        .map_err(|(reason, _)| Failure::new(reason, None))?;
    let mut page = Page::attach(Cdp::listening(socket, EVENTS), &request.display_id, true).await?;
    let top = page.top.clone();
    call(&mut page, "Runtime.enable", json!({})).await?;
    call(
        &mut page,
        "Runtime.addBinding",
        json!({"name": BINDING, "executionContextName": WORLD}),
    )
    .await?;
    let mut world = inject(&mut page, &request.strings).await?;
    let mut shown = (request.pins.clone(), request.full, request.strings.clone());
    draw(&mut page, world, &shown).await;
    let _ = frames.send(frame(&request.key(), true, None)).await;
    let ended = loop {
        tokio::select! {
            // Taken or dropped alike: Comment off, or the screen went.
            _ = &mut off => break Ok(()),
            control = controls.recv() => match control {
                Some(Pins { pins, full, strings }) => {
                    if strings != shown.2 {
                        eval(&mut page, world, &format!("__hideComment.start({strings})")).await;
                    }
                    shown = (pins, full, strings);
                    draw(&mut page, world, &shown).await;
                }
                None => break Ok(()),
            },
            event = page.cdp.next_event() => {
                let event = match event {
                    Ok(event) => event,
                    Err(error) => break Err(page.failure(error)),
                };
                match event.method.as_str() {
                    "Page.frameNavigated"
                        if event.session.as_deref() == Some(top.as_str())
                            && event.params["frame"].get("parentId").is_none() =>
                    {
                        // A new document: the overlay goes in again, with the
                        // pins of the address it is at now.
                        match inject(&mut page, &shown.2).await {
                            Ok(id) => {
                                world = id;
                                draw(&mut page, world, &shown).await;
                            }
                            Err(failure) => break Err(failure),
                        }
                    }
                    "Runtime.bindingCalled"
                        if event.params["name"] == BINDING
                            && event.params["executionContextId"].as_i64() == Some(world) =>
                    {
                        let message = event.params["payload"].as_str().unwrap_or("");
                        if message.len() > MAX_MESSAGE_BYTES {
                            continue;
                        }
                        let Ok(message) = serde_json::from_str::<Value>(message) else {
                            continue;
                        };
                        if message["type"] == "off" {
                            break Ok(());
                        }
                        report(context, &request, &mut page, world, message).await;
                    }
                    "Inspector.detached" => break Err(Failure::new("display_missing", None)),
                    _ => {}
                }
            }
        }
    };
    eval(&mut page, world, "__hideComment.stop()").await;
    page.close().await;
    ended
}

async fn call(page: &mut Page, method: &str, params: Value) -> Result<Value, Failure> {
    let top = page.top.clone();
    page.cdp
        .call(method, params, Some(&top), CALL)
        .await
        .map_err(|error| page.failure(error))
}

/// A fresh isolated world on the page's document, with the overlay in it.
async fn inject(page: &mut Page, strings: &Value) -> Result<i64, Failure> {
    let tree = call(page, "Page.getFrameTree", json!({})).await?;
    let world = call(
        page,
        "Page.createIsolatedWorld",
        json!({"frameId": tree["frameTree"]["frame"]["id"], "worldName": WORLD}),
    )
    .await?;
    let id = world["executionContextId"]
        .as_i64()
        .ok_or_else(|| Failure::new("display_missing", None))?;
    let answer = call(
        page,
        "Runtime.evaluate",
        json!({"expression": format!("{COMMENT_JS};__hideComment.start({strings})"), "contextId": id, "returnByValue": true}),
    )
    .await?;
    if answer.get("exceptionDetails").is_some() {
        return Err(Failure::new("page_script_failed", None));
    }
    Ok(id)
}

async fn eval(page: &mut Page, world: i64, expression: &str) {
    let _ = call(
        page,
        "Runtime.evaluate",
        json!({"expression": expression, "contextId": world, "returnByValue": true}),
    )
    .await;
}

async fn draw(page: &mut Page, world: i64, shown: &(Value, bool, Value)) {
    eval(
        page,
        world,
        &format!("__hideComment.pins({}, {})", shown.0, shown.1),
    )
    .await;
}

/// One overlay message: a new comment with its screenshot, an edit or a
/// delete, each the core's `view_comment` event.
async fn report(context: &Context, request: &Request, page: &mut Page, world: i64, message: Value) {
    let workspace = json!({"device_id": request.device_id, "path": request.path});
    let payload = match message["type"].as_str() {
        Some("add") => {
            let anchor = message["anchor"].clone();
            let shot = match shot(page, &anchor["rect"]).await {
                Some(png) => {
                    let attachments = Arc::clone(&context.attachments);
                    tokio::task::spawn_blocking(move || attachments.stage_view_shot(&png))
                        .await
                        .ok()
                        .and_then(|staged| {
                            staged
                                .map_err(|error| {
                                    herdr_core::diagnostic!(json!({
                                        "component": "browser_comment",
                                        "kind": "shot.stage_failed",
                                        "display_id": request.display_id,
                                        "error": error,
                                    }));
                                })
                                .ok()
                        })
                }
                None => None,
            };
            let mut payload = json!({
                "workspace": workspace, "action": "add", "display_id": request.display_id,
                "anchor": anchor, "note": message["note"],
            });
            if let Some(name) = shot {
                payload["shot"] = json!({"node": context.core.node().as_str(), "name": name});
            }
            payload
        }
        Some("edit") => json!({
            "workspace": workspace, "action": "edit",
            "comment_id": message["id"], "note": message["note"],
        }),
        Some("delete") => json!({
            "workspace": workspace, "action": "delete", "comment_id": message["id"],
        }),
        _ => return,
    };
    let event = json!({"schema_version": 2, "kind": "view_comment", "payload": payload});
    let core = Arc::clone(&context.core);
    let dispatched =
        tokio::task::spawn_blocking(move || core.dispatch(event.to_string().into_bytes()))
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result);
    if let Err(error) = &dispatched {
        herdr_core::diagnostic!(json!({
            "component": "browser_comment",
            "kind": "comment.dispatch_failed",
            "display_id": request.display_id,
            "error": error,
        }));
    }
    if message["type"] == "add" {
        eval(
            page,
            world,
            &format!("__hideComment.saved({})", dispatched.is_ok()),
        )
        .await;
    }
}

/// The page clipped to `rect` (document CSS pixels), scaled down past
/// `MAX_SHOT_PIXELS`; none when the page gives none.
async fn shot(page: &mut Page, rect: &Value) -> Option<Vec<u8>> {
    let read = |key: &str| rect[key].as_f64().filter(|value| value.is_finite());
    let (x, y, width, height) = (read("x")?, read("y")?, read("width")?, read("height")?);
    if width < 1.0 || height < 1.0 {
        return None;
    }
    let scale = (MAX_SHOT_PIXELS / (width * height)).sqrt().min(1.0);
    let answer = call(
        page,
        "Page.captureScreenshot",
        json!({"format": "png", "clip": {"x": x, "y": y, "width": width, "height": height, "scale": scale}}),
    )
    .await;
    let data = match answer {
        Ok(answer) => answer["data"].as_str().map(str::to_owned)?,
        Err(failure) => {
            herdr_core::diagnostic!(json!({
                "component": "browser_comment",
                "kind": "shot.capture_failed",
                "reason": failure.reason,
            }));
            return None;
        }
    };
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}
