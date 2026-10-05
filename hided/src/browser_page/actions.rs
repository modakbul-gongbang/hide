//! What each `hide browser` command does on an attached page.

use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};
use tokio::time::{Instant, sleep};

use super::cdp::CdpError;
use super::page::{Frame, Page, STEP};
use super::{Command, DragMode, Failure, Output, Target, Verify, Wait};

const CONSOLE_LIMIT: usize = 50;
const NETWORK_LIMIT: usize = 100;
const EVAL_TIMEOUT: Duration = Duration::from_secs(9);
const WAIT_POLL: Duration = Duration::from_millis(100);
/// Three actions in a row that changed nothing usually mean a dead control,
/// an overlay taking the click, or a loop (chromux STALL_STREAK_THRESHOLD).
const STALL_STREAK: u64 = 3;
const DIALOG_NEXT: &str = "The page opened a dialog; the operator must answer it in hide, and the next command fails dialog_open until then";

/// The keys `press` sends (chromux KEY_DEFS).
const KEYS: &[(&str, u32, Option<&str>)] = &[
    ("Enter", 13, Some("\r")),
    ("Tab", 9, None),
    ("Escape", 27, None),
    ("Backspace", 8, None),
    ("Delete", 46, None),
    ("ArrowUp", 38, None),
    ("ArrowDown", 40, None),
    ("ArrowLeft", 37, None),
    ("ArrowRight", 39, None),
    ("Home", 36, None),
    ("End", 35, None),
    ("PageUp", 33, None),
    ("PageDown", 34, None),
];

/// A place an input event goes: the session of the frame that holds it, the
/// point in that frame's viewport, and the same point in the top viewport
/// when it is known (for the overlay).
struct Point {
    session: String,
    x: f64,
    y: f64,
    top: Option<(f64, f64)>,
    opaque_frame: bool,
    draggable: bool,
    label: Value,
}

fn selector(number: u64) -> String {
    format!("[data-ct-ref=\"{number}\"]")
}

fn ref_text(target: &Target) -> Value {
    match target {
        Target::Ref { tag: None, number } => json!(format!("@{number}")),
        Target::Ref {
            tag: Some(tag),
            number,
        } => json!(format!("@{tag}:{number}")),
        Target::Text(text) => json!({"text": text}),
        Target::Point { x, y, image } => {
            json!({"xy": [x, y], "space": if *image { "image" } else { "css" }})
        }
    }
}

pub async fn run(page: &mut Page, command: Command) -> Result<Output, Failure> {
    let display = page.display.clone();
    let next = json!(format!("hide browser snapshot {display} --diff"));
    match command {
        Command::Help => unreachable!("help runs without a page"),
        Command::Snapshot {
            interactive,
            diff,
            grep,
            ..
        } => {
            let filter = interactive.then_some("interactive");
            let composite = page.snapshot(filter, "auto").await?;
            let key = if interactive { "interactive" } else { "full" };
            let previous = page.swap_baseline(key, &composite).await?;
            let text = composite.text();
            let shown = if diff {
                page.render("diff", json!({"previous": previous, "current": text}))
                    .await?
            } else if let Some(pattern) = grep {
                page.render("grep", json!({"text": text, "pattern": pattern}))
                    .await?
            } else {
                json!(text)
            };
            Ok(Output::Text(shown.as_str().unwrap_or("").to_owned()))
        }
        Command::Click { target, verify, .. } => {
            page.require_visible().await?;
            let before = capture(page, &verify).await?;
            let point = locate(page, &target, true).await?;
            glide(page, &point).await;
            mouse(page, &point, "mouseMoved", "none", 0).await?;
            // Entering a cross-origin frame can take one renderer turn before
            // a press is routed into it (chromux).
            if point.opaque_frame {
                sleep(Duration::from_millis(100)).await;
            }
            mouse(page, &point, "mousePressed", "left", 1).await?;
            mouse(page, &point, "mouseReleased", "left", 0).await?;
            if let Some((x, y)) = point.top {
                page.overlay("click", json!({"x": x, "y": y})).await;
            }
            let mut answer = json!({"ok": true, "clicked": point.label});
            finish(page, &verify, before, acted(&target), &mut answer, next).await;
            Ok(Output::Json(answer))
        }
        Command::Fill {
            target,
            text,
            verify,
            ..
        } => {
            let (session, sel, offset) = element(page, &target).await?;
            let before = capture(page, &verify).await?;
            let filled = page
                .dom(&session, "fill", json!({"selector": sel, "text": text}))
                .await?;
            let mut answer = json!({"ok": true, "filled": ref_text(&target)});
            if filled["contenteditable"] == true {
                page.require_visible().await?;
                input(page, &session, "Input.insertText", json!({"text": text})).await?;
                if page.cdp.dialog.is_none() {
                    let observed = page
                        .dom(&session, "editableText", json!({"selector": sel}))
                        .await?;
                    if observed["text"].as_str() != Some(text.as_str()) {
                        return Err(Failure::new(
                            "fill_rejected",
                            Some(format!(
                                "expected {}, observed {}",
                                json!(text),
                                observed["text"]
                            )),
                        ));
                    }
                }
            } else if let Some(label) = filled.get("selectedLabel") {
                answer["selected"] = json!({"value": filled["value"], "label": label});
            }
            if let Ok(rect) = page.dom(&session, "box", json!({"selector": sel})).await {
                flash(page, &rect, offset).await;
            }
            finish(page, &verify, before, acted(&target), &mut answer, next).await;
            Ok(Output::Json(answer))
        }
        Command::Type { text, verify, .. } => {
            page.require_visible().await?;
            let before = capture(page, &verify).await?;
            let (session, offset) = focused(page).await?;
            input(page, &session, "Input.insertText", json!({"text": text})).await?;
            if let Ok(rect) = page.dom(&session, "focusRect", json!({})).await
                && !rect["rect"].is_null()
            {
                flash(page, &rect["rect"], offset).await;
            }
            let mut answer = json!({"ok": true, "typed": text});
            finish(page, &verify, before, None, &mut answer, next).await;
            Ok(Output::Json(answer))
        }
        Command::Press { key, verify, .. } => {
            let Some((name, code, text)) = KEYS.iter().find(|(name, ..)| *name == key) else {
                return Err(Failure::new(
                    "key_unsupported",
                    Some(
                        KEYS.iter()
                            .map(|(name, ..)| *name)
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                ));
            };
            page.require_visible().await?;
            let before = capture(page, &verify).await?;
            let (session, offset) = focused(page).await?;
            let mut down = json!({"type": "keyDown", "key": name, "code": name,
                "windowsVirtualKeyCode": code, "nativeVirtualKeyCode": code});
            if let Some(text) = text {
                down["text"] = json!(text);
            }
            input(page, &session, "Input.dispatchKeyEvent", down).await?;
            input(
                page,
                &session,
                "Input.dispatchKeyEvent",
                json!({"type": "keyUp", "key": name,
                "code": name, "windowsVirtualKeyCode": code, "nativeVirtualKeyCode": code}),
            )
            .await?;
            let rect = page.dom(&session, "focusRect", json!({})).await.ok();
            let rect = rect
                .and_then(|rect| (!rect["rect"].is_null()).then(|| shift(&rect["rect"], offset)));
            page.overlay("key", json!({"name": name, "rect": rect}))
                .await;
            let mut answer = json!({"ok": true, "pressed": name});
            finish(page, &verify, before, None, &mut answer, next).await;
            Ok(Output::Json(answer))
        }
        Command::Hover { target, verify, .. } => {
            page.require_visible().await?;
            let before = capture(page, &verify).await?;
            let point = locate(page, &target, true).await?;
            glide(page, &point).await;
            mouse(page, &point, "mouseMoved", "none", 0).await?;
            let mut answer = json!({"ok": true, "hovered": point.label});
            finish(page, &verify, before, None, &mut answer, next).await;
            Ok(Output::Json(answer))
        }
        Command::Drag {
            from,
            to,
            mode,
            verify,
            ..
        } => {
            page.require_visible().await?;
            let before = capture(page, &verify).await?;
            // Bring both ends into view first, then measure both again without
            // scrolling, so no point from before a scroll is dispatched.
            let both_refs = matches!(from, Target::Ref { .. }) && matches!(to, Target::Ref { .. });
            locate(page, &from, both_refs).await?;
            locate(page, &to, both_refs).await?;
            let start = locate(page, &from, false).await?;
            let end = locate(page, &to, false).await?;
            if start.session != end.session {
                return Err(Failure::new("drag_across_frames", None));
            }
            let html5 = match mode {
                DragMode::Auto => start.draggable,
                DragMode::Pointer => false,
                DragMode::Html5 => true,
            };
            glide(page, &start).await;
            const STEPS: u32 = 12;
            if let (Some(a), Some(b)) = (start.top, end.top) {
                page.overlay(
                    "drag",
                    json!({"x1": a.0, "y1": a.1, "x2": b.0, "y2": b.1, "ms": STEPS * 16 + 100}),
                )
                .await;
            }
            drag(page, &start, &end, STEPS, html5).await?;
            let mut answer = json!({"ok": true, "dragged": {"from": start.label, "to": end.label,
                "mode": if html5 { "html5" } else { "pointer" }}});
            finish(page, &verify, before, None, &mut answer, next).await;
            Ok(Output::Json(answer))
        }
        Command::Scroll {
            direction,
            target,
            verify,
            ..
        } => {
            let before = capture(page, &verify).await?;
            let mut answer = json!({"ok": true});
            match (direction, target) {
                (Some(up), _) => {
                    let probe = page.require_visible().await?;
                    let (width, height) = (
                        probe["width"].as_f64().unwrap_or(0.0),
                        probe["height"].as_f64().unwrap_or(0.0),
                    );
                    let top = page.top.clone();
                    let start = probe["scroll"]["y"].as_f64().unwrap_or(0.0);
                    let delta = (height * 0.85).round().max(1.0);
                    input(page, &top, "Input.dispatchMouseEvent", json!({"type": "mouseWheel",
                        "x": width / 2.0, "y": height / 2.0, "deltaX": 0, "deltaY": if up { -delta } else { delta }}))
                    .await?;
                    page.overlay(
                        "scroll",
                        json!({"direction": if up { "up" } else { "down" }}),
                    )
                    .await;
                    answer["scrolled"] = json!(if up { "up" } else { "down" });
                    if let Ok(after) = page.dom(&top, "scrollY", json!({})).await {
                        answer["moved"] = json!(after["y"].as_f64().unwrap_or(start) - start);
                    }
                }
                (None, Some(target)) => {
                    let (session, sel, _) = element(page, &target).await?;
                    let moved = page
                        .dom(&session, "scrollIntoView", json!({"selector": sel}))
                        .await?;
                    let dy = moved["dy"].as_f64().unwrap_or(0.0);
                    if dy != 0.0 {
                        page.overlay(
                            "scroll",
                            json!({"direction": if dy < 0.0 { "up" } else { "down" }}),
                        )
                        .await;
                    }
                    answer["scrolled"] = ref_text(&target);
                    answer["moved"] = json!(dy);
                }
                (None, None) => unreachable!("parse requires a direction or a target"),
            }
            finish(page, &verify, before, None, &mut answer, next).await;
            Ok(Output::Json(answer))
        }
        Command::Wait { wait, timeout, .. } => wait_for(page, wait, timeout).await,
        Command::Screenshot { path, target, .. } => screenshot(page, &path, target.as_ref()).await,
        Command::Eval { expression, .. } => {
            let top = page.top.clone();
            let answer = page
                .cdp
                .call(
                    "Runtime.evaluate",
                    json!({"expression": expression, "returnByValue": true, "awaitPromise": true}),
                    Some(&top),
                    EVAL_TIMEOUT,
                )
                .await
                .map_err(|error| page.blocked(error))?;
            if let Some(details) = answer.get("exceptionDetails") {
                return Err(Failure::new(
                    "eval_error",
                    Some(super::page::first_line(details)),
                ));
            }
            let result = &answer["result"];
            let mut out =
                json!({"ok": true, "value": result.get("value").cloned().unwrap_or(Value::Null)});
            if result.get("value").is_none() {
                out["type"] = result["type"].clone();
            }
            Ok(Output::Json(out))
        }
        Command::Console { .. } => console(page).await,
        Command::Network { .. } => {
            let top = page.top.clone();
            let rows = page
                .dom(&top, "network", json!({"limit": NETWORK_LIMIT}))
                .await?;
            Ok(Output::Text(network_text(&rows)))
        }
    }
}

fn acted(target: &Target) -> Option<String> {
    match target {
        Target::Ref { .. } => ref_text(target).as_str().map(str::to_owned),
        _ => None,
    }
}

/// The frame session and selector an element ref names, and the frame's
/// offset in the top viewport when it is a cross-origin frame.
async fn element(
    page: &mut Page,
    target: &Target,
) -> Result<(String, String, Option<(f64, f64)>), Failure> {
    match target {
        Target::Ref { tag: None, number } => Ok((page.top.clone(), selector(*number), None)),
        Target::Ref {
            tag: Some(tag),
            number,
        } => {
            let frame = page.frame(tag).await?;
            let offset = page.frame_offset(&frame).await.ok();
            Ok((frame.session, selector(*number), offset))
        }
        _ => Err(Failure::new("ref_required", None)),
    }
}

async fn locate(page: &mut Page, target: &Target, scroll: bool) -> Result<Point, Failure> {
    match target {
        Target::Ref { tag, number } => {
            let (session, frame) = match tag {
                None => (page.top.clone(), None::<Frame>),
                Some(tag) => {
                    let frame = page.frame(tag).await?;
                    (frame.session.clone(), Some(frame))
                }
            };
            let rect = page
                .dom(
                    &session,
                    "rect",
                    json!({"selector": selector(*number), "scroll": scroll}),
                )
                .await?;
            let (x, y) = (
                rect["centerX"].as_f64().unwrap_or(0.0),
                rect["centerY"].as_f64().unwrap_or(0.0),
            );
            let top = match &frame {
                None => Some((x, y)),
                Some(frame) => page
                    .frame_offset(frame)
                    .await
                    .ok()
                    .map(|(dx, dy)| (x + dx, y + dy)),
            };
            Ok(Point {
                session,
                x,
                y,
                top,
                opaque_frame: rect["opaqueFrame"] == true,
                draggable: rect["draggable"] == true,
                label: ref_text(target),
            })
        }
        Target::Text(label) => {
            let top = page.top.clone();
            let found = page.dom(&top, "textTarget", json!({"text": label})).await?;
            let number = found["ref"].as_u64().unwrap_or(0);
            let mut point =
                Box::pin(locate(page, &Target::Ref { tag: None, number }, scroll)).await?;
            point.label = json!({"text": label, "ref": format!("@{number}")});
            Ok(point)
        }
        Target::Point { x, y, image } => {
            let probe = page.probe().await?;
            let (x, y) = if *image {
                let (width, height) = screenshot_size(page).await?;
                if *x < 0.0 || *y < 0.0 || *x >= width || *y >= height {
                    return Err(Failure::new(
                        "outside_viewport",
                        Some(format!("image point {x},{y} outside {width}x{height}")),
                    ));
                }
                let vv = &probe["vv"];
                (
                    vv["left"].as_f64().unwrap_or(0.0)
                        + x * vv["width"].as_f64().unwrap_or(width) / width,
                    vv["top"].as_f64().unwrap_or(0.0)
                        + y * vv["height"].as_f64().unwrap_or(height) / height,
                )
            } else {
                (*x, *y)
            };
            let (width, height) = (
                probe["width"].as_f64().unwrap_or(0.0),
                probe["height"].as_f64().unwrap_or(0.0),
            );
            if x < 0.0 || y < 0.0 || x >= width || y >= height {
                return Err(Failure::new(
                    "outside_viewport",
                    Some(format!("css point {x},{y} outside {width}x{height}")),
                ));
            }
            Ok(Point {
                session: page.top.clone(),
                x,
                y,
                top: Some((x, y)),
                opaque_frame: false,
                draggable: false,
                label: ref_text(target),
            })
        }
    }
}

/// The frame session that holds the keyboard focus: the top document, or the
/// cross-origin frame its focused iframe leads to.
async fn focused(page: &mut Page) -> Result<(String, Option<(f64, f64)>), Failure> {
    let top = page.top.clone();
    let opaque = page.dom(&top, "activeOpaqueFrame", json!({})).await?;
    if opaque["opaque"] != true {
        return Ok((top, None));
    }
    for frame in page.frames().await? {
        if let Ok(focus) = page.dom(&frame.session, "hasFocus", json!({})).await
            && focus["focus"] == true
        {
            let offset = page.frame_offset(&frame).await.ok();
            return Ok((frame.session, offset));
        }
    }
    Ok((top, None))
}

/// Moves the operator's view of the cursor to the point and waits for the
/// glide, so the press lands where the cursor arrived.
async fn glide(page: &mut Page, point: &Point) {
    let Some((x, y)) = point.top else { return };
    if let Some(answer) = page.overlay("move", json!({"x": x, "y": y})).await {
        let ms = answer["ms"].as_u64().unwrap_or(0).min(500);
        sleep(Duration::from_millis(ms)).await;
    }
}

fn shift(rect: &Value, offset: Option<(f64, f64)>) -> Value {
    let (dx, dy) = offset.unwrap_or((0.0, 0.0));
    json!({"x": rect["x"].as_f64().unwrap_or(0.0) + dx, "y": rect["y"].as_f64().unwrap_or(0.0) + dy,
        "width": rect["width"], "height": rect["height"]})
}

async fn flash(page: &mut Page, rect: &Value, offset: Option<(f64, f64)>) {
    page.overlay("flash", shift(rect, offset)).await;
}

async fn mouse(
    page: &mut Page,
    point: &Point,
    kind: &str,
    button: &str,
    buttons: u32,
) -> Result<(), Failure> {
    let mut event =
        json!({"type": kind, "x": point.x, "y": point.y, "button": button, "pointerType": "mouse"});
    if kind != "mouseMoved" {
        event["buttons"] = json!(buttons);
        event["clickCount"] = json!(1);
    }
    let session = point.session.clone();
    input(page, &session, "Input.dispatchMouseEvent", event).await
}

/// Dispatches one input event. A dialog the event opened is reported by the
/// command, not treated as a failure.
async fn input(page: &mut Page, session: &str, method: &str, params: Value) -> Result<(), Failure> {
    match page
        .cdp
        .call_input(method, params, Some(session), STEP)
        .await
    {
        Ok(_) => Ok(()),
        Err(CdpError::Timeout) => {
            let probe = page.probe().await;
            if probe.is_ok_and(|probe| probe["visibility"] == "hidden") {
                Err(Failure::new("display_hidden", None))
            } else {
                Err(Failure::new("page_unresponsive", None))
            }
        }
        Err(error) => Err(page.failure(error)),
    }
}

async fn drag(
    page: &mut Page,
    start: &Point,
    end: &Point,
    steps: u32,
    html5: bool,
) -> Result<(), Failure> {
    let session = start.session.clone();
    if html5 {
        page.cdp
            .call(
                "Input.setInterceptDrags",
                json!({"enabled": true}),
                Some(&session),
                STEP,
            )
            .await
            .map_err(|error| page.blocked(error))?;
    }
    let result = drag_moves(page, start, end, steps, html5).await;
    if html5 {
        let _ = page
            .cdp
            .call(
                "Input.setInterceptDrags",
                json!({"enabled": false}),
                Some(&session),
                STEP,
            )
            .await;
    }
    result
}

async fn drag_moves(
    page: &mut Page,
    start: &Point,
    end: &Point,
    steps: u32,
    html5: bool,
) -> Result<(), Failure> {
    let session = start.session.clone();
    mouse(page, start, "mouseMoved", "none", 0).await?;
    mouse(page, start, "mousePressed", "left", 1).await?;
    let mut pressed_at = (start.x, start.y);
    let moved = async {
        sleep(Duration::from_millis(100)).await;
        for index in 1..=steps {
            let progress = f64::from(index) / f64::from(steps);
            pressed_at = (
                start.x + (end.x - start.x) * progress,
                start.y + (end.y - start.y) * progress,
            );
            input(page, &session, "Input.dispatchMouseEvent", json!({"type": "mouseMoved",
                "x": pressed_at.0, "y": pressed_at.1, "button": "left", "buttons": 1, "pointerType": "mouse"}))
            .await?;
            sleep(Duration::from_millis(16)).await;
        }
        if html5 {
            let data = intercepted_drag(page, &session).await?;
            for kind in ["dragEnter", "dragOver", "drop"] {
                input(page, &session, "Input.dispatchDragEvent", json!({"type": kind, "x": end.x, "y": end.y, "data": data}))
                    .await?;
            }
        }
        Ok::<(), Failure>(())
    }
    .await;
    // The button is released on every path, so the page never keeps a drag.
    let release = input(page, &session, "Input.dispatchMouseEvent", json!({"type": "mouseReleased",
        "x": pressed_at.0, "y": pressed_at.1, "button": "left", "buttons": 0, "clickCount": 1, "pointerType": "mouse"}))
    .await;
    moved.and(release)
}

/// Chromium hands a native HTML5 drag to the client instead of starting it.
async fn intercepted_drag(page: &mut Page, session: &str) -> Result<Value, Failure> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let found = page
            .cdp
            .take_events(|event| event.method == "Input.dragIntercepted");
        if let Some(event) = found.into_iter().next() {
            return Ok(event.params["data"].clone());
        }
        if Instant::now() >= deadline {
            return Err(Failure::new(
                "drag_not_started",
                Some("the source did not start a native drag; try --mode pointer".into()),
            ));
        }
        page.flush(session).await?;
        sleep(Duration::from_millis(50)).await;
    }
}

/// The page before an action, read the way `changed` compares it: every
/// frame, with clickable detection capped in document order so scrolling
/// alone never reads as a change.
async fn capture(page: &mut Page, verify: &Verify) -> Result<Option<String>, Failure> {
    match verify {
        Verify::Off => Ok(None),
        Verify::After(_) => Ok(Some(page.snapshot(None, "stable").await?.text())),
    }
}

/// Adds what the action changed (or a dialog it opened) to its answer.
async fn finish(
    page: &mut Page,
    verify: &Verify,
    before: Option<String>,
    acted: Option<String>,
    answer: &mut Value,
    next: Value,
) {
    if let Some(dialog) = page.cdp.dialog.clone() {
        answer["dialog"] = dialog;
        answer["next_action"] = json!(DIALOG_NEXT);
        return;
    }
    answer["next"] = next;
    let (Verify::After(wait), Some(before)) = (verify, before) else {
        return;
    };
    match changed(page, *wait, &before, acted).await {
        Ok(text) => answer["changed"] = json!(text),
        Err(_) if page.cdp.dialog.is_some() => {
            if let Some(object) = answer.as_object_mut() {
                object.remove("next");
            }
            answer["dialog"] = page.cdp.dialog.clone().unwrap_or(Value::Null);
            answer["next_action"] = json!(DIALOG_NEXT);
        }
        Err(failure) => {
            answer["changed"] = json!(format!(
                "# changed: the page could not be read after the action ({}); run snapshot --diff before repeating it",
                failure.reason
            ));
        }
    }
}

async fn changed(
    page: &mut Page,
    wait: Duration,
    before: &str,
    acted: Option<String>,
) -> Result<String, Failure> {
    sleep(wait).await;
    let mut current = page.snapshot(None, "stable").await?.text();
    let first = page
        .render(
            "changes",
            json!({"previous": before, "current": current, "ref": acted}),
        )
        .await?;
    let none = first["count"].as_u64() == Some(0);
    if none || first["selfEchoOnly"] == true {
        sleep(Duration::from_millis(700)).await;
        current = page.snapshot(None, "stable").await?.text();
    }
    if none {
        let again = page
            .render("changes", json!({"previous": before, "current": current}))
            .await?;
        if again["count"].as_u64() == Some(0) {
            // Slow pages land their update seconds later; say so in time
            // terms, since a bare "no change" pushes a retry (a double submit).
            sleep(Duration::from_millis(1200)).await;
            current = page.snapshot(None, "stable").await?.text();
            let late = page
                .render("changes", json!({"previous": before, "current": current}))
                .await?;
            if late["count"].as_u64() == Some(0) {
                let top = page.top.clone();
                let streak = page.dom(&top, "streak", json!({"changed": false})).await?;
                let streak = streak["streak"].as_u64().unwrap_or(1);
                let mut message = "# changed: no visible change within ~2s — the action was dispatched, but the page may still be updating or the result may be in another display or a dialog; confirm with hide browser wait or snapshot --diff BEFORE repeating the action".to_owned();
                if streak >= STALL_STREAK {
                    message.push_str(&format!("\n# stalled: {streak} actions in a row changed nothing — you are likely stuck (a dead control, an overlay taking the click, or a loop). Do not repeat it; try another element, dismiss the overlay, wait for the state you expect, or hand off to the operator."));
                }
                return Ok(message);
            }
        }
    }
    let top = page.top.clone();
    page.dom(&top, "streak", json!({"changed": true})).await?;
    let text = page
        .render("changed", json!({"previous": before, "current": current}))
        .await?;
    Ok(text.as_str().unwrap_or("").to_owned())
}

async fn wait_for(page: &mut Page, wait: Wait, timeout: Duration) -> Result<Output, Failure> {
    let started = Instant::now();
    let deadline = started + timeout;
    loop {
        let found = match &wait {
            Wait::Text(text) => {
                let mut sessions = vec![page.top.clone()];
                sessions.extend(page.frames().await?.into_iter().map(|frame| frame.session));
                let mut found = false;
                for session in sessions {
                    if let Ok(answer) = page.dom(&session, "waitText", json!({"text": text})).await
                        && answer["found"] == true
                    {
                        found = true;
                        break;
                    }
                }
                found
            }
            Wait::Selector { selector, gone } => {
                let visible = match super::parse_ref(selector) {
                    Some(Target::Ref {
                        tag: Some(tag),
                        number,
                    }) => match page.frame(&tag).await {
                        Ok(frame) => {
                            page.dom(
                                &frame.session,
                                "waitSelector",
                                json!({"selector": self::selector(number)}),
                            )
                            .await?["found"]
                                == true
                        }
                        // The frame's document is gone, so is its element.
                        Err(_) => false,
                    },
                    Some(Target::Ref { tag: None, number }) => {
                        let top = page.top.clone();
                        page.dom(
                            &top,
                            "waitSelector",
                            json!({"selector": self::selector(number)}),
                        )
                        .await?["found"]
                            == true
                    }
                    _ => {
                        let top = page.top.clone();
                        page.dom(&top, "waitSelector", json!({"selector": selector}))
                            .await?["found"]
                            == true
                    }
                };
                visible != *gone
            }
        };
        let waited = started.elapsed().as_millis() as u64;
        if found {
            let answer = match &wait {
                Wait::Text(text) => json!({"ok": true, "found_text": text, "waited_ms": waited}),
                Wait::Selector {
                    selector,
                    gone: false,
                } => {
                    json!({"ok": true, "found_selector": selector, "waited_ms": waited})
                }
                Wait::Selector {
                    selector,
                    gone: true,
                } => {
                    json!({"ok": true, "gone_selector": selector, "waited_ms": waited})
                }
            };
            return Ok(Output::Json(answer));
        }
        if Instant::now() + WAIT_POLL > deadline {
            let what = match &wait {
                Wait::Text(text) => format!("text {} not found", json!(text)),
                Wait::Selector {
                    selector,
                    gone: false,
                } => format!("{selector} not visible"),
                Wait::Selector {
                    selector,
                    gone: true,
                } => format!("{selector} still visible"),
            };
            return Err(Failure::new(
                "timeout",
                Some(format!("{what} after {} ms", timeout.as_millis())),
            ));
        }
        sleep(WAIT_POLL).await;
    }
}

/// The size of a full screenshot now, which `--space image` points refer to.
async fn screenshot_size(page: &mut Page) -> Result<(f64, f64), Failure> {
    let bytes = capture_png(page, None).await?;
    let (width, height) =
        png_size(&bytes).ok_or_else(|| Failure::new("screenshot_failed", None))?;
    Ok((f64::from(width), f64::from(height)))
}

async fn capture_png(page: &mut Page, clip: Option<Value>) -> Result<Vec<u8>, Failure> {
    let top = page.top.clone();
    let mut params = json!({"format": "png"});
    if let Some(clip) = clip {
        params["clip"] = clip;
    }
    let shot = page
        .cdp
        .call("Page.captureScreenshot", params, Some(&top), STEP)
        .await
        .map_err(|error| page.blocked(error))?;
    base64::engine::general_purpose::STANDARD
        .decode(shot["data"].as_str().unwrap_or(""))
        .map_err(|_| Failure::new("screenshot_failed", None))
}

pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[1..4] != b"PNG" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

async fn screenshot(
    page: &mut Page,
    path: &str,
    target: Option<&Target>,
) -> Result<Output, Failure> {
    let file = std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| PathBuf::from(path));
    page.require_visible().await?;
    let mut crop = None;
    if let Some(target) = target {
        let (session, sel, offset) = element(page, target).await?;
        page.dom(&session, "scrollIntoView", json!({"selector": sel}))
            .await?;
        let rect = shift(
            &page.dom(&session, "box", json!({"selector": sel})).await?,
            offset,
        );
        crop = Some(rect);
    }
    // Measured after any scroll the ref needed.
    let probe = page.probe().await?;
    let vv = &probe["vv"];
    let view = (
        vv["left"].as_f64().unwrap_or(0.0),
        vv["top"].as_f64().unwrap_or(0.0),
        vv["width"].as_f64().unwrap_or(0.0),
        vv["height"].as_f64().unwrap_or(0.0),
    );
    let css = match &crop {
        None => view,
        Some(rect) => {
            let (x, y) = (
                rect["x"].as_f64().unwrap_or(0.0),
                rect["y"].as_f64().unwrap_or(0.0),
            );
            let (w, h) = (
                rect["width"].as_f64().unwrap_or(0.0),
                rect["height"].as_f64().unwrap_or(0.0),
            );
            let left = x.max(view.0);
            let top = y.max(view.1);
            let right = (x + w).min(view.0 + view.2);
            let bottom = (y + h).min(view.1 + view.3);
            if right <= left || bottom <= top {
                return Err(Failure::new("target_outside_viewport", None));
            }
            (left, top, right - left, bottom - top)
        }
    };
    let clip = crop.as_ref().map(|_| {
        json!({"x": probe["scroll"]["x"].as_f64().unwrap_or(0.0) + css.0,
            "y": probe["scroll"]["y"].as_f64().unwrap_or(0.0) + css.1,
            "width": css.2, "height": css.3, "scale": 1})
    });
    let bytes = capture_png(page, clip).await?;
    let (width, height) =
        png_size(&bytes).ok_or_else(|| Failure::new("screenshot_failed", None))?;
    std::fs::write(&file, &bytes).map_err(|error| {
        Failure::new(
            "screenshot_unwritable",
            Some(format!("{}: {error}", file.display())),
        )
    })?;
    let mut answer = json!({
        "ok": true,
        "path": file.display().to_string(),
        "image": {"width": width, "height": height},
        "css": {"x": css.0, "y": css.1, "width": css.2, "height": css.3},
        "css_to_image": {"scale_x": f64::from(width) / css.2, "scale_y": f64::from(height) / css.3,
            "offset_css_x": css.0, "offset_css_y": css.1},
    });
    if let Some(target) = target {
        answer["ref"] = ref_text(target);
    }
    Ok(Output::Json(answer))
}

/// Chrome replays a document's stored console messages to a session that
/// enables the Runtime domain, so a stateless command still reads them.
async fn console(page: &mut Page) -> Result<Output, Failure> {
    let top = page.top.clone();
    page.cdp
        .call("Runtime.enable", json!({}), Some(&top), STEP)
        .await
        .map_err(|error| page.blocked(error))?;
    page.flush(&top).await?;
    let events = page.cdp.take_events(|event| {
        event.session.as_deref() == Some(top.as_str())
            && matches!(
                event.method.as_str(),
                "Runtime.consoleAPICalled" | "Runtime.exceptionThrown"
            )
    });
    let lines: Vec<String> = events
        .iter()
        .rev()
        .map(|event| console_line(&event.params, &event.method))
        .collect();
    Ok(Output::Text(console_text(&lines)))
}

fn console_line(params: &Value, method: &str) -> String {
    let (level, text, frame) = if method == "Runtime.exceptionThrown" {
        let details = &params["exceptionDetails"];
        (
            "exception".to_owned(),
            super::page::first_line(details),
            details["stackTrace"]["callFrames"][0].clone(),
        )
    } else {
        let text = params["args"]
            .as_array()
            .map(|args| {
                args.iter()
                    .map(|arg| match &arg["value"] {
                        Value::String(text) => text.clone(),
                        Value::Null => arg["description"]
                            .as_str()
                            .or_else(|| arg["unserializableValue"].as_str())
                            .or_else(|| arg["type"].as_str())
                            .unwrap_or("")
                            .to_owned(),
                        value => value.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        (
            params["type"].as_str().unwrap_or("log").to_owned(),
            text,
            params["stackTrace"]["callFrames"][0].clone(),
        )
    };
    let text: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(500)
        .collect();
    let location = frame["url"]
        .as_str()
        .filter(|url| !url.is_empty())
        .map(|url| {
            format!(
                "  ({url}:{}:{})",
                frame["lineNumber"].as_u64().unwrap_or(0) + 1,
                frame["columnNumber"].as_u64().unwrap_or(0) + 1
            )
        })
        .unwrap_or_default();
    format!("[{level}] {text}{location}")
}

fn console_text(lines: &[String]) -> String {
    if lines.is_empty() {
        return "# console: no messages in the current document\n".to_owned();
    }
    let shown = lines.len().min(CONSOLE_LIMIT);
    let mut out = format!(
        "# console: {} message{} in the current document, newest first{}\n\n",
        lines.len(),
        if lines.len() == 1 { "" } else { "s" },
        if shown < lines.len() {
            format!(", showing {shown}")
        } else {
            String::new()
        }
    );
    for line in &lines[..shown] {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn network_text(rows: &Value) -> String {
    let total = rows["total"].as_u64().unwrap_or(0);
    let list = rows["rows"].as_array().cloned().unwrap_or_default();
    let mut out = format!(
        "# network: {total} resource{} loaded by the current document (resource timing), newest first{}\n# request and response headers and bodies are not available\n",
        if total == 1 { "" } else { "s" },
        if (list.len() as u64) < total {
            format!(", showing {}", list.len())
        } else {
            String::new()
        }
    );
    if list.is_empty() {
        return out;
    }
    out.push('\n');
    for row in list {
        let status = match row["status"].as_u64() {
            Some(0) | None => "-".to_owned(),
            Some(status) => status.to_string(),
        };
        out.push_str(&format!(
            "{status} {}ms {}B {} {}\n",
            row["ms"].as_u64().unwrap_or(0),
            row["bytes"].as_u64().unwrap_or(0),
            row["type"].as_str().unwrap_or(""),
            row["url"].as_str().unwrap_or("")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_header_gives_its_pixel_size() {
        let mut bytes = vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
        ];
        bytes.extend_from_slice(&2048u32.to_be_bytes());
        bytes.extend_from_slice(&1280u32.to_be_bytes());
        assert_eq!(png_size(&bytes), Some((2048, 1280)));
        assert_eq!(png_size(b"GIF89a................."), None);
        assert_eq!(png_size(&bytes[..20]), None);
    }

    #[test]
    fn console_lines_name_level_text_and_place_newest_first() {
        let log = json!({"type": "warning", "args": [{"type": "string", "value": "slow\n  load"}, {"type": "number", "value": 3}],
            "stackTrace": {"callFrames": [{"url": "http://127.0.0.1:3000/app.js", "lineNumber": 9, "columnNumber": 4}]}});
        assert_eq!(
            console_line(&log, "Runtime.consoleAPICalled"),
            "[warning] slow load 3  (http://127.0.0.1:3000/app.js:10:5)"
        );
        let thrown = json!({"exceptionDetails": {"text": "Uncaught", "exception": {"description": "TypeError: x is undefined\n    at f"}}});
        assert_eq!(
            console_line(&thrown, "Runtime.exceptionThrown"),
            "[exception] TypeError: x is undefined"
        );
        assert_eq!(
            console_text(&[]),
            "# console: no messages in the current document\n"
        );
        let many: Vec<String> = (0..60).map(|n| format!("[log] {n}")).collect();
        let text = console_text(&many);
        assert!(text.starts_with(
            "# console: 60 messages in the current document, newest first, showing 50\n"
        ));
        assert_eq!(text.lines().count(), 52);
    }

    #[test]
    fn network_lines_never_claim_headers_or_bodies() {
        let rows = json!({"total": 2, "rows": [
            {"url": "http://127.0.0.1:3000/app.js", "type": "script", "status": 200, "ms": 12, "bytes": 3400},
            {"url": "https://cdn.example/x.png", "type": "img", "status": 0, "ms": 40, "bytes": 0}]});
        let text = network_text(&rows);
        assert!(text.contains("# request and response headers and bodies are not available\n"));
        assert!(text.contains("200 12ms 3400B script http://127.0.0.1:3000/app.js\n"));
        assert!(text.contains("- 40ms 0B img https://cdn.example/x.png\n"));
    }
}
