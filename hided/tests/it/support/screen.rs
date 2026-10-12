//! A screen of a daemon's machine, as the web shell opens one: its `/ws`,
//! the frames it reads, and keys typed into a pane through it. The journeys
//! of the node role and of a core move both type through it.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use super::remote_delivery::PROMPT;

pub type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A screen of the daemon at `port`: its `/ws`, as the web shell opens it.
pub async fn screen_socket(port: u16, token: &str) -> Result<Socket> {
    screen_socket_of(port, token, "web").await
}

/// The same screen as a client of `kind` opens it.
pub async fn screen_socket_of(port: u16, token: &str, kind: &str) -> Result<Socket> {
    let mut request = format!("ws://127.0.0.1:{port}/ws").into_client_request()?;
    request
        .headers_mut()
        .insert("origin", format!("http://127.0.0.1:{port}").parse()?);
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await?;
    socket
        .send(Message::Text(
            json!({"token": token, "schema_version": 2, "client_kind": kind})
                .to_string()
                .into(),
        ))
        .await?;
    Ok(socket)
}

pub async fn send(socket: &mut Socket, kind: &str, payload: Value) -> Result<()> {
    socket
        .send(Message::Text(
            json!({"schema_version": 2, "kind": kind, "payload": payload})
                .to_string()
                .into(),
        ))
        .await?;
    Ok(())
}

/// The next text frame, or `None` once the deadline passed.
pub async fn next_frame(socket: &mut Socket, deadline: Instant) -> Result<Option<Value>> {
    loop {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return Ok(None);
        };
        let Ok(message) = tokio::time::timeout(left, socket.next()).await else {
            return Ok(None);
        };
        match message {
            Some(Ok(Message::Text(text))) => return Ok(Some(serde_json::from_str(&text)?)),
            Some(Ok(Message::Close(frame))) => bail!("the screen socket closed: {frame:?}"),
            Some(Ok(_)) => {}
            Some(Err(error)) => bail!("the screen socket failed: {error}"),
            None => bail!("the screen socket ended"),
        }
    }
}

/// The first snapshot frame `socket` receives within `bound`.
pub async fn first_snapshot(socket: &mut Socket, bound: Duration) -> Result<Value> {
    let deadline = Instant::now() + bound;
    loop {
        let frame = next_frame(socket, deadline)
            .await?
            .context("no snapshot reached the screen")?;
        if frame["type"] == "snapshot" {
            return Ok(frame);
        }
    }
}

/// Every `{pane_id, bytes_base64}` chunk of a `terminal` frame, decoded.
pub fn chunks(frame: &Value) -> Vec<(String, Vec<u8>)> {
    if frame["type"] != "terminal" {
        return Vec::new();
    }
    frame["payload"]["chunks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|chunk| {
            let pane = chunk["pane_id"].as_str()?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(chunk["bytes_base64"].as_str()?)
                .ok()?;
            Some((pane.to_owned(), bytes))
        })
        .collect()
}

/// The screen's text without its control sequences.
pub fn plain(screen: &str) -> String {
    let mut text = String::new();
    let mut chars = screen.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if !c.is_control() || c == '\n' {
                text.push(c);
            }
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    text
}

/// Types `line` into `pane` once its shell drew its prompt, and reads the screen
/// until `answer` appears on it; answers what the pane drew after the
/// keys, without its control sequences.
pub async fn type_and_read(
    socket: &mut Socket,
    pane: &str,
    line: &str,
    answer: &str,
) -> Result<String> {
    for kind in ["terminal_viewport", "terminal_resize"] {
        send(
            socket,
            kind,
            json!({"pane_id": pane, "cols": 100, "rows": 30, "new_view": true}),
        )
        .await?;
    }
    let mut screen = String::new();
    let mut typed = false;
    let deadline = Instant::now() + Duration::from_secs(60);
    while let Some(frame) = next_frame(socket, deadline).await? {
        for (from, bytes) in chunks(&frame) {
            if from == pane {
                screen.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        if !typed && plain(&screen).contains(PROMPT.trim_end()) {
            let keys = base64::engine::general_purpose::STANDARD.encode(format!("{line}\r"));
            send(
                socket,
                "key",
                json!({"pane_id": pane, "bytes_base64": keys}),
            )
            .await?;
            typed = true;
            screen.clear();
        }
        if typed && plain(&screen).contains(answer) {
            return Ok(plain(&screen));
        }
    }
    bail!(
        "{answer} never came back on {pane} (typed: {typed}); the screen read: {:?}",
        plain(&screen)
    )
}
