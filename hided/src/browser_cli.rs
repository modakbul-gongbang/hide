//! `hide browser open` (issue 155): asks the running hide to show a page in a
//! View area. The request travels the same token-gated WebSocket the shell
//! uses, as one `browser_open` event, and the core answers it with a receipt
//! in `status.browser_opens` that carries the request's id; a path the
//! daemon's boundary refuses is answered with `path_refused` instead.

use std::path::Path;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::ORIGIN;

use crate::state_file::DaemonState;

/// How long the CLI waits for the core's answer before it gives up.
const ANSWER_WITHIN: Duration = Duration::from_secs(10);
const SCHEMA_VERSION: u32 = 2;

/// What the argument asks to load: a URL with a scheme as written, a file or
/// folder that exists as its `file:` URL, and anything else as a host, over
/// http when it is a loopback address (a dev server) and https otherwise.
pub fn address(target: &str, cwd: &Path) -> Result<String, String> {
    let text = target.trim();
    if text.is_empty() {
        return Err("nothing to open".to_owned());
    }
    if has_scheme(text) {
        return Ok(text.to_owned());
    }
    let path = cwd.join(text);
    if path.exists() {
        let real = path
            .canonicalize()
            .map_err(|error| format!("{text}: {error}"))?;
        return Ok(crate::file_url::file_url(&real.display().to_string(), ""));
    }
    if text.starts_with(['/', '.', '~']) {
        return Err(format!("no such file: {text}"));
    }
    if text.chars().any(char::is_whitespace) {
        return Err(format!("not an address: {text}"));
    }
    let scheme = if is_loopback(text) { "http" } else { "https" };
    Ok(format!("{scheme}://{text}"))
}

/// Whether `text` starts with a URL scheme, which `localhost:3000` (a host
/// and a port) does not.
fn has_scheme(text: &str) -> bool {
    let Some((scheme, rest)) = text.split_once(':') else {
        return false;
    };
    let named = scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    let port = rest.split(['/', '?', '#']).next().unwrap_or("");
    named && !(!port.is_empty() && port.chars().all(|c| c.is_ascii_digit()))
}

fn is_loopback(text: &str) -> bool {
    let host = if text.starts_with('[') {
        text.split_inclusive(']').next().unwrap_or("")
    } else {
        text.split([':', '/', '?', '#']).next().unwrap_or("")
    };
    let host = host.to_ascii_lowercase();
    host == "localhost"
        || host == "0.0.0.0"
        || host == "[::1]"
        || host.strip_prefix("127.").is_some_and(|rest| {
            rest.split('.').count() == 3 && rest.split('.').all(|part| part.parse::<u8>().is_ok())
        })
}

/// The request's `browser_open` payload: the pane that asked, when there is
/// one, lets the core open the page in that pane's Workspace.
pub fn payload(url: &str, pane: Option<&str>, request_id: &str) -> Value {
    let mut payload = json!({"url": url, "request_id": request_id});
    if let Some(pane) = pane {
        payload["pane_id"] = Value::String(pane.to_owned());
    }
    payload
}

/// Sends one `browser_open` and waits for its answer: the core's receipt,
/// or the boundary's refusal. Either is the one JSON line the CLI prints.
pub async fn request(state: &DaemonState, payload: Value) -> Result<Value, String> {
    let request_id = payload["request_id"].as_str().unwrap_or("").to_owned();
    tokio::time::timeout(ANSWER_WITHIN, exchange(state, payload, &request_id))
        .await
        .map_err(|_| format!("hide did not answer within {}s", ANSWER_WITHIN.as_secs()))?
}

async fn exchange(state: &DaemonState, payload: Value, request_id: &str) -> Result<Value, String> {
    let mut request = format!("ws://127.0.0.1:{}/ws", state.port)
        .into_client_request()
        .map_err(|error| error.to_string())?;
    let origin = format!("http://127.0.0.1:{}", state.port);
    request.headers_mut().insert(
        ORIGIN,
        origin.parse().map_err(|_| "origin header".to_owned())?,
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|error| format!("connect: {error}"))?;
    let handshake = json!({"token": state.token, "schema_version": SCHEMA_VERSION});
    socket
        .send(Message::Text(handshake.to_string().into()))
        .await
        .map_err(|error| format!("handshake: {error}"))?;
    let mut sent = false;
    while let Some(message) = socket.next().await {
        let text = match message.map_err(|error| format!("read: {error}"))? {
            Message::Text(text) => text,
            Message::Close(frame) => {
                let reason = frame
                    .map(|frame| format!("{} {}", u16::from(frame.code), frame.reason))
                    .unwrap_or_default();
                return Err(format!("hide closed the connection {reason}")
                    .trim()
                    .to_owned());
            }
            _ => continue,
        };
        let frame: Value =
            serde_json::from_str(&text).map_err(|error| format!("frame: {error}"))?;
        // The daemon describes itself once the handshake is accepted; the
        // event goes after it so it is never read as the handshake.
        if !sent {
            if frame["type"] == "daemon" {
                let event = json!({"schema_version": SCHEMA_VERSION, "kind": "browser_open", "payload": payload});
                socket
                    .send(Message::Text(event.to_string().into()))
                    .await
                    .map_err(|error| format!("send: {error}"))?;
                sent = true;
            }
            continue;
        }
        if let Some(answer) = answer(&frame, request_id) {
            let _ = socket.close(None).await;
            return Ok(answer);
        }
    }
    Err("hide closed the connection".to_owned())
}

/// The answer to request `request_id` a frame carries, if it carries one.
fn answer(frame: &Value, request_id: &str) -> Option<Value> {
    match frame["type"].as_str()? {
        "path_refused" if frame["payload"]["kind"] == "browser_open" => Some(json!({
            "ok": false,
            "reason": frame["payload"]["reason"],
            "path": frame["payload"]["path"],
        })),
        "error" => Some(json!({"ok": false, "reason": "error", "message": frame["message"]})),
        "snapshot" | "delta" => frame
            .pointer("/payload/rest/status/browser_opens")?
            .as_array()?
            .iter()
            .find(|receipt| receipt["request_id"] == request_id)
            .cloned(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_argument_becomes_the_address_it_names() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("보고서 1.html"), "<p>ok</p>").unwrap();
        let real = dir.path().canonicalize().unwrap();
        let file = crate::file_url::file_url(&real.join("보고서 1.html").display().to_string(), "");
        assert_eq!(address("보고서 1.html", dir.path()), Ok(file.clone()));
        assert_eq!(
            address(
                &real.join("보고서 1.html").display().to_string(),
                Path::new("/")
            ),
            Ok(file)
        );
        for (input, expected) in [
            ("https://example.com/a?b#c", "https://example.com/a?b#c"),
            ("about:blank", "about:blank"),
            ("localhost:3000", "http://localhost:3000"),
            ("127.0.0.1:8080/docs", "http://127.0.0.1:8080/docs"),
            ("[::1]:5173", "http://[::1]:5173"),
            ("example.com", "https://example.com"),
            ("docs.rs/serde", "https://docs.rs/serde"),
        ] {
            assert_eq!(
                address(input, dir.path()).as_deref(),
                Ok(expected),
                "{input}"
            );
        }
        for input in ["", "./missing.html", "/no/such/file.html", "two words"] {
            assert!(address(input, dir.path()).is_err(), "{input}");
        }
    }

    #[test]
    fn only_its_own_receipt_or_a_refusal_answers_the_request() {
        let receipt = json!({"request_id": "r1", "ok": true, "display_id": "d1"});
        let delta = json!({"type": "delta", "payload": {"rest": {"status": {"browser_opens": [
            {"request_id": "r0", "ok": false}, receipt,
        ]}}}});
        assert_eq!(answer(&delta, "r1"), Some(receipt));
        assert_eq!(answer(&delta, "r2"), None);
        assert_eq!(
            answer(&json!({"type": "delta", "payload": {"terminal": []}}), "r1"),
            None
        );
        let refused = json!({"type": "path_refused", "payload": {"kind": "browser_open", "path": "/x", "reason": "outside_checkout"}});
        assert_eq!(
            answer(&refused, "r1").unwrap()["reason"],
            "outside_checkout"
        );
        let other = json!({"type": "path_refused", "payload": {"kind": "file_list", "path": "/x", "reason": "outside_checkout"}});
        assert_eq!(answer(&other, "r1"), None);
    }
}
