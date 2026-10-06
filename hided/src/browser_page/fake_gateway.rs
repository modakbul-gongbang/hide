//! A scripted gateway on a local socket for the tests of the page commands:
//! each request is answered by the messages a script returns, events first,
//! and none leaves it silent, the way a page held by a script is.

use std::time::Duration;

use serde_json::{Value, json};

use super::cdp::Cdp;
use super::page::Page;

/// What a script returns to close the socket with a code, as the relay does
/// when it sees no answer for a minute.
pub fn close(code: u16) -> Value {
    json!({"__close": code})
}

pub async fn gateway(mut script: impl FnMut(&Value) -> Vec<Value> + Send + 'static) -> Cdp {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        while let Some(Ok(message)) = futures_util::StreamExt::next(&mut socket).await {
            let Ok(text) = message.into_text() else {
                continue;
            };
            let request: Value = serde_json::from_str(text.as_str()).unwrap();
            for reply in script(&request) {
                use tokio_tungstenite::tungstenite::Message;
                use tokio_tungstenite::tungstenite::protocol::CloseFrame;
                let message = match reply["__close"].as_u64() {
                    Some(code) => Message::Close(Some(CloseFrame {
                        code: u16::try_from(code).unwrap().into(),
                        reason: "".into(),
                    })),
                    None => Message::text(reply.to_string()),
                };
                if futures_util::SinkExt::send(&mut socket, message)
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    });
    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}"))
        .await
        .unwrap();
    Cdp::new(socket)
}

/// A page whose calls wait this long for an answer, not the production step.
pub const STEP: Duration = Duration::from_millis(500);

pub async fn page(script: impl FnMut(&Value) -> Vec<Value> + Send + 'static) -> Page {
    Page::for_test(gateway(script).await, STEP)
}

pub fn reply(request: &Value, result: Value) -> Value {
    let mut answer = json!({"id": request["id"], "result": result});
    if let Some(session) = request["sessionId"].as_str() {
        answer["sessionId"] = json!(session);
    }
    answer
}

pub fn attached(session: &str, parent: &str, url: &str) -> Value {
    json!({"method": "Target.attachedToTarget", "sessionId": parent,
        "params": {"sessionId": session, "targetInfo": {"type": "iframe", "targetId": format!("t-{session}"), "url": url}}})
}

/// How the frames of a scripted page behave.
#[derive(Clone, Copy, Default)]
pub struct Frames {
    /// Frames `f1`..`fN` that never answer `Page.enable`.
    pub hung: usize,
    /// Healthy frames `ok1`..`okN`, with tags `k7q2`, `k7q3`, ...
    pub healthy: usize,
    /// A healthy frame answers a kind of request (`Page.enable`, its tag, its
    /// snapshot, its baseline) only once every healthy frame has asked, so a
    /// client that sends one request and waits for its answer before the next
    /// never gets one.
    pub together: bool,
    /// The healthy frames answer everything but their snapshot read.
    pub stops_on_read: bool,
    /// The first healthy frame answers everything but its baseline swap.
    pub stops_on_baseline: bool,
    /// The first healthy frame leaves its first `waitText` unanswered, then
    /// finds the text.
    pub slow_once_on_wait: bool,
    /// What the top document answers a `waitText` with.
    pub wait_found: bool,
}

/// A top document with `hung` frames that never answer `Page.enable` and
/// `healthy` frames. `before` is sent ahead of the first hung frame's
/// silence, the way a dialog event reaches a client that listens.
pub fn page_with_frames(
    frames: Frames,
    before: Option<Value>,
) -> impl FnMut(&Value) -> Vec<Value> + Send {
    let mut held: std::collections::HashMap<&'static str, Vec<Value>> = Default::default();
    let mut waits = 0;
    move |request| {
        let session = request["sessionId"].as_str().unwrap_or("");
        let expression = request["params"]["expression"].as_str().unwrap_or("");
        let healthy = session
            .strip_prefix("ok")
            .and_then(|n| n.parse::<usize>().ok());
        let mut answer = |kind: &'static str, answer: Value| -> Vec<Value> {
            if !frames.together {
                return vec![answer];
            }
            let waiting = held.entry(kind).or_default();
            waiting.push(answer);
            if waiting.len() < frames.healthy {
                return Vec::new();
            }
            std::mem::take(waiting)
        };
        match (request["method"].as_str().unwrap(), session) {
            ("Target.setAutoAttach", "top") => {
                let mut messages: Vec<Value> = (1..=frames.hung)
                    .map(|n| {
                        attached(
                            &format!("f{n}"),
                            "top",
                            &format!("HTTP://Hung{n}.test:80/ad"),
                        )
                    })
                    .collect();
                messages.extend(
                    (1..=frames.healthy)
                        .map(|n| attached(&format!("ok{n}"), "top", "http://ok.test:9/child")),
                );
                messages.push(reply(request, json!({})));
                messages
            }
            ("Target.setAutoAttach", _) => vec![reply(request, json!({}))],
            ("Page.enable", "f1") => before.clone().into_iter().collect(),
            ("Page.enable", session) if session.starts_with('f') => Vec::new(),
            ("Page.enable", _) => answer("enable", reply(request, json!({}))),
            ("Runtime.evaluate", _) if expression == "0" => vec![reply(
                request,
                json!({"result": {"type": "number", "value": 0}}),
            )],
            ("Runtime.evaluate", "top") if expression.contains("\"waitText\"") => vec![reply(
                request,
                json!({"result": {"value": {"found": frames.wait_found}}}),
            )],
            ("Runtime.evaluate", "top") if expression.contains("\"baseline\"") => vec![reply(
                request,
                json!({"result": {"value": {"previous": "# T\n# http://a/\n"}}}),
            )],
            ("Runtime.evaluate", "top") => vec![reply(
                request,
                json!({"result": {"value": "# T\n# http://a/\n\n@1 button \"A\"\n"}}),
            )],
            ("Runtime.evaluate", _) if healthy.is_some() => {
                let n = healthy.unwrap_or(1);
                if expression.ends_with("(\"tag\",{})") {
                    let tag = format!("k7q{}", n + 1);
                    return answer(
                        "tag",
                        reply(
                            request,
                            json!({"result": {"value": {"tag": tag, "origin": "http://ok.test:9"}}}),
                        ),
                    );
                }
                if expression.contains("\"baseline\"") {
                    if frames.stops_on_baseline && n == 1 {
                        return Vec::new();
                    }
                    return answer(
                        "baseline",
                        reply(
                            request,
                            json!({"result": {"value": {"previous": format!("# OOPIF k7q{} origin=http://ok.test:9\nold", n + 1)}}}),
                        ),
                    );
                }
                if expression.contains("\"waitSelector\"") {
                    if frames.stops_on_read {
                        return Vec::new();
                    }
                    return vec![reply(
                        request,
                        json!({"result": {"value": {"found": true}}}),
                    )];
                }
                if expression.contains("\"waitText\"") {
                    waits += 1;
                    if frames.stops_on_read || frames.slow_once_on_wait && n == 1 && waits == 1 {
                        return Vec::new();
                    }
                    return vec![reply(
                        request,
                        json!({"result": {"value": {"found": frames.slow_once_on_wait}}}),
                    )];
                }
                if frames.stops_on_read {
                    return Vec::new();
                }
                answer(
                    "read",
                    reply(
                        request,
                        json!({"result": {"value": "# Child\n# http://ok.test:9/child\n\n@1 link \"B\"\n"}}),
                    ),
                )
            }
            _ => Vec::new(),
        }
    }
}
