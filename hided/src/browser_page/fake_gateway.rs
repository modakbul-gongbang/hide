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
    /// A healthy frame `ok` that answers everything but its snapshot read.
    pub stops_on_read: bool,
    /// What the top document answers a `waitText` with.
    pub wait_found: bool,
}

/// A top document with `hung` frames that never answer `Page.enable` and a
/// healthy frame `ok`. `before` is sent ahead of the first hung frame's
/// silence, the way a dialog event reaches a client that listens.
pub fn page_with_frames(
    frames: Frames,
    before: Option<Value>,
) -> impl FnMut(&Value) -> Vec<Value> + Send {
    move |request| {
        let session = request["sessionId"].as_str().unwrap_or("");
        let expression = request["params"]["expression"].as_str().unwrap_or("");
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
                messages.push(attached("ok", "top", "http://ok.test:9/child"));
                messages.push(reply(request, json!({})));
                messages
            }
            ("Target.setAutoAttach", _) => vec![reply(request, json!({}))],
            ("Page.enable", "f1") => before.clone().into_iter().collect(),
            ("Page.enable", session) if session.starts_with('f') => Vec::new(),
            ("Page.enable", _) => vec![reply(request, json!({}))],
            ("Runtime.evaluate", "top") if expression == "0" => vec![reply(
                request,
                json!({"result": {"type": "number", "value": 0}}),
            )],
            ("Runtime.evaluate", "top") if expression.contains("\"waitText\"") => vec![reply(
                request,
                json!({"result": {"value": {"found": frames.wait_found}}}),
            )],
            ("Runtime.evaluate", "top") => vec![reply(
                request,
                json!({"result": {"value": "# T\n# http://a/\n\n@1 button \"A\"\n"}}),
            )],
            ("Runtime.evaluate", "ok") if expression.ends_with("(\"tag\",{})") => vec![reply(
                request,
                json!({"result": {"value": {"tag": "k7q2", "origin": "http://ok.test:9"}}}),
            )],
            ("Runtime.evaluate", "ok") if frames.stops_on_read => Vec::new(),
            ("Runtime.evaluate", "ok") if expression.contains("\"waitText\"") => vec![reply(
                request,
                json!({"result": {"value": {"found": false}}}),
            )],
            ("Runtime.evaluate", "ok") => vec![reply(
                request,
                json!({"result": {"value": "# Child\n# http://ok.test:9/child\n\n@1 link \"B\"\n"}}),
            )],
            _ => Vec::new(),
        }
    }
}
