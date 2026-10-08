//! A scripted gateway on a local socket for the tests of the page commands:
//! each request is answered by the messages a script returns, events first,
//! and none leaves it silent, the way a page held by a script is.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::cdp::Cdp;
use super::page::Page;

/// What a script returns to close the socket with a code, as the relay does
/// when it sees no answer for a minute.
pub fn close(code: u16) -> Value {
    json!({"__close": code})
}

/// What a script returns to send a reply some time after the request, as the
/// real gateway answers a command its page never does: an error at its own
/// deadline.
pub fn after(delay: Duration, reply: Value) -> Value {
    let mut reply = reply;
    reply["__after_ms"] = json!(u64::try_from(delay.as_millis()).unwrap());
    reply
}

pub async fn gateway(mut script: impl FnMut(&Value) -> Vec<Value> + Send + 'static) -> Cdp {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::protocol::CloseFrame;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        // Replies that are due later, with when.
        let mut later: Vec<(tokio::time::Instant, Value)> = Vec::new();
        loop {
            let due = later.iter().map(|(at, _)| *at).min();
            let mut replies = Vec::new();
            tokio::select! {
                message = socket.next() => {
                    let Some(Ok(message)) = message else { return };
                    let Ok(text) = message.into_text() else {
                        continue;
                    };
                    let request: Value = serde_json::from_str(text.as_str()).unwrap();
                    for reply in script(&request) {
                        match reply["__after_ms"].as_u64() {
                            Some(ms) => later.push((
                                tokio::time::Instant::now() + Duration::from_millis(ms),
                                reply,
                            )),
                            None => replies.push(reply),
                        }
                    }
                }
                () = async {
                    match due {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => {
                    let now = tokio::time::Instant::now();
                    let (ready, rest): (Vec<_>, Vec<_>) =
                        later.drain(..).partition(|(at, _)| *at <= now);
                    later = rest;
                    replies.extend(ready.into_iter().map(|(_, reply)| reply));
                }
            }
            for mut reply in replies {
                reply
                    .as_object_mut()
                    .map(|object| object.remove("__after_ms"));
                let message = match reply["__close"].as_u64() {
                    Some(code) => Message::Close(Some(CloseFrame {
                        code: u16::try_from(code).unwrap().into(),
                        reason: "".into(),
                    })),
                    None => Message::text(reply.to_string()),
                };
                if socket.send(message).await.is_err() {
                    return;
                }
            }
        }
    });
    let stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let transport: std::pin::Pin<Box<dyn crate::workspace_cli::Transport>> = Box::pin(stream);
    let (socket, _) = tokio_tungstenite::client_async(format!("ws://{address}"), transport)
        .await
        .unwrap();
    Cdp::new(socket)
}

/// A page whose calls wait this long for an answer, not the production step.
pub const STEP: Duration = Duration::from_millis(500);

/// How long the scripted gateway keeps a command nobody answers pending
/// before it answers with an error, the way the real one does at ten seconds
/// against the production step of eight.
pub const HOLD: Duration = Duration::from_millis(625);

pub async fn page(script: impl FnMut(&Value) -> Vec<Value> + Send + 'static) -> Page {
    page_within(script, STEP).await
}

/// A page whose calls wait `step` for an answer: a long one lets a test see
/// that a read is bounded by something other than the step.
pub async fn page_within(
    script: impl FnMut(&Value) -> Vec<Value> + Send + 'static,
    step: Duration,
) -> Page {
    Page::for_test(gateway(script).await, step)
}

/// What the scripted gateway saw: every request, and the most commands it
/// held at once.
#[derive(Default)]
pub struct Traffic {
    requests: Vec<(String, String)>,
    pub most_pending: usize,
    /// Set by a test of `recording_held`: the gateway ends what it holds with
    /// the next request it receives.
    pub release: bool,
}

impl Traffic {
    /// How many requests of a method went to a session.
    pub fn sent(&self, method: &str, session: &str) -> usize {
        self.requests
            .iter()
            .filter(|(sent, to)| sent == method && to == session)
            .count()
    }
}

pub type Seen = Arc<Mutex<Traffic>>;

/// Wraps a script with the gateway's own bookkeeping: a command no reply
/// answers stays pending until `HOLD` after it arrived, when it is answered
/// with an error, and one that arrives while 32 are pending closes the
/// connection with 1013, as the real gateway does.
pub fn recording(
    script: impl FnMut(&Value) -> Vec<Value> + Send + 'static,
) -> (impl FnMut(&Value) -> Vec<Value> + Send + 'static, Seen) {
    recording_by(script, Some(HOLD))
}

/// As `recording`, but the gateway ends the commands nobody answers when the
/// test says so (`Traffic::release`), not after a time: the test orders the
/// end of what is held against the reads it makes, and nothing waits.
/// The error answers go out ahead of the answer to the next request the
/// gateway receives.
pub fn recording_held(
    script: impl FnMut(&Value) -> Vec<Value> + Send + 'static,
) -> (impl FnMut(&Value) -> Vec<Value> + Send + 'static, Seen) {
    recording_by(script, None)
}

fn recording_by(
    mut script: impl FnMut(&Value) -> Vec<Value> + Send + 'static,
    hold: Option<Duration>,
) -> (impl FnMut(&Value) -> Vec<Value> + Send + 'static, Seen) {
    let seen = Seen::default();
    let log = Arc::clone(&seen);
    // When each held command arrived, and the answer that ends it once released.
    let mut pending: Vec<(Instant, Value)> = Vec::new();
    let script = move |request: &Value| {
        let now = Instant::now();
        let mut seen = log.lock().unwrap();
        let mut ended = Vec::new();
        match hold {
            Some(hold) => pending.retain(|(arrived, _)| now < *arrived + hold),
            None if std::mem::take(&mut seen.release) => {
                ended = pending.drain(..).map(|(_, error)| error).collect();
            }
            None => {}
        }
        seen.requests.push((
            request["method"].as_str().unwrap_or("").to_owned(),
            request["sessionId"].as_str().unwrap_or("").to_owned(),
        ));
        if pending.len() >= 32 {
            return vec![close(1013)];
        }
        seen.most_pending = seen.most_pending.max(pending.len() + 1);
        let mut replies = script(request);
        if !replies.iter().any(|reply| reply["id"] == request["id"]) {
            let error = json!({"id": request["id"], "sessionId": request["sessionId"],
                "error": {"message": "CDP command timed out"}});
            match hold {
                Some(hold) => {
                    replies.push(after(hold, error.clone()));
                    pending.push((now, error));
                }
                None => pending.push((now, error)),
            }
        }
        // The error answers precede the answer to this request on the socket,
        // so a client that has read its answer has read them.
        ended.extend(replies);
        ended
    };
    (script, seen)
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
    /// Whether the first healthy frame finds the text a `waitText` asks for
    /// (the others never do).
    pub frame_wait_found: bool,
    /// The first healthy frame leaves its first `Page.enable` unanswered.
    pub slow_enable_once: bool,
    /// The first healthy frame leaves its first snapshot read unanswered.
    pub slow_read_once: bool,
    /// The first healthy frame leaves every other snapshot read unanswered,
    /// the first included.
    pub flaky_reads: bool,
    /// The first healthy frame answers everything but the look for its own
    /// frames (`Target.setAutoAttach`).
    pub scan_silent: bool,
    /// The top document reports that a cross-origin frame holds the focus.
    pub opaque_focus: bool,
    /// The healthy frame `okN` that holds the focus (0: none does).
    pub focus_in: usize,
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
    let mut enables_of_first = 0;
    let mut reads_of_first = 0;
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
            ("Target.setAutoAttach", "ok1") if frames.scan_silent => Vec::new(),
            ("Target.setAutoAttach", _) => vec![reply(request, json!({}))],
            ("Page.enable", "f1") => before.clone().into_iter().collect(),
            ("Page.enable", session) if session.starts_with('f') => Vec::new(),
            ("Page.enable", _) => {
                if healthy == Some(1) {
                    enables_of_first += 1;
                    if frames.slow_enable_once && enables_of_first == 1 {
                        return Vec::new();
                    }
                }
                answer("enable", reply(request, json!({})))
            }
            ("Runtime.evaluate", _) if expression == "0" => vec![reply(
                request,
                json!({"result": {"type": "number", "value": 0}}),
            )],
            ("Runtime.evaluate", "top") if expression.contains("\"activeOpaqueFrame\"") => {
                vec![reply(
                    request,
                    json!({"result": {"value": {"opaque": frames.opaque_focus}}}),
                )]
            }
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
                if expression.contains("\"hasFocus\"") {
                    if frames.stops_on_read {
                        return Vec::new();
                    }
                    return vec![reply(
                        request,
                        json!({"result": {"value": {"focus": frames.focus_in == n}}}),
                    )];
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
                    let found = frames.slow_once_on_wait || frames.frame_wait_found && n == 1;
                    return answer(
                        "wait",
                        reply(request, json!({"result": {"value": {"found": found}}})),
                    );
                }
                if n == 1 {
                    reads_of_first += 1;
                    if frames.slow_read_once && reads_of_first == 1
                        || frames.flaky_reads && reads_of_first % 2 == 1
                    {
                        return Vec::new();
                    }
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
