//! The CDP relay behind `hide browser`: a pane's CLI, local or on a connected
//! device (through the reverse Workspace forward), reaches the desktop's
//! scoped gateway only through hided, so the gateway's loopback address and
//! capability URL never leave this process. The relay runs on its own task,
//! outside the core's lock, and touches no core state.

use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{self, Message as GatewayMessage};

use crate::browser_control::Failure;

/// The gateway's own message cap (`browserCdp.ts`, `MAX_MESSAGE_BYTES`): a
/// larger frame could not be legitimate in either direction.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
/// A command polls or answers well within this; a silent pair is abandoned.
const IDLE: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Close codes the CLI reads as a reason (RFC 6455 4000-4999 are private).
pub const CLOSE_MESSAGE_LIMIT: u16 = 4009;
pub const CLOSE_IDLE: u16 = 4008;
pub const CLOSE_GATEWAY_LOST: u16 = 4011;

type Gateway =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Opens the gateway side before the CLI is told the relay is ready, so a
/// refused upgrade is an answer the caller can act on, not a dropped socket.
pub async fn connect(browser_ws_url: &str) -> Result<Gateway, Failure> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES));
    let connected = tokio::time::timeout(
        CONNECT_TIMEOUT,
        tokio_tungstenite::connect_async_with_config(browser_ws_url, Some(config), false),
    )
    .await;
    match connected {
        Ok(Ok((socket, _))) => Ok(socket),
        Ok(Err(tungstenite::Error::Http(response))) if response.status().as_u16() == 429 => Err((
            "browser_control_busy",
            "Close another CDP client of this desktop window and retry",
        )),
        _ => Err((
            "browser_control_unavailable",
            "Reconnect the Hide desktop app and retry",
        )),
    }
}

/// Moves text frames both ways until either side closes, a frame crosses the
/// cap, or both stay silent past the idle bound. Every exit closes both.
pub async fn pump(client: &mut WebSocket, mut gateway: Gateway, display_id: &str) {
    let mut deadline = tokio::time::Instant::now() + IDLE;
    let (code, reason) = loop {
        tokio::select! {
            incoming = client.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if text.len() > MAX_MESSAGE_BYTES {
                        break (CLOSE_MESSAGE_LIMIT, "browser_relay_message_limit");
                    }
                    if gateway.send(GatewayMessage::Text(text.as_str().into())).await.is_err() {
                        break (CLOSE_GATEWAY_LOST, "browser_relay_gateway_lost");
                    }
                }
                Some(Ok(Message::Binary(_))) => break (CLOSE_MESSAGE_LIMIT, "browser_relay_text_only"),
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break (1000, "client_closed"),
                Some(Ok(_)) => {}
            },
            outgoing = gateway.next() => match outgoing {
                Some(Ok(GatewayMessage::Text(text))) => {
                    if client.send(Message::Text(text.as_str().into())).await.is_err() {
                        break (1000, "client_closed");
                    }
                }
                Some(Ok(GatewayMessage::Close(frame))) => {
                    // The gateway's own reason (a closed display, a resource
                    // limit) reaches the CLI unchanged.
                    let (code, text) = frame.map_or((1000, String::new()), |frame| (u16::from(frame.code), frame.reason.to_string()));
                    let _ = client.send(Message::Close(Some(CloseFrame { code, reason: text.into() }))).await;
                    if code != 1000 {
                        log(display_id, "gateway_closed", code);
                    }
                    return;
                }
                Some(Err(tungstenite::Error::Capacity(_))) => break (CLOSE_MESSAGE_LIMIT, "browser_relay_message_limit"),
                Some(Ok(_)) => {}
                None | Some(Err(_)) => break (CLOSE_GATEWAY_LOST, "browser_relay_gateway_lost"),
            },
            () = tokio::time::sleep_until(deadline) => break (CLOSE_IDLE, "browser_relay_idle"),
        }
        deadline = tokio::time::Instant::now() + IDLE;
    };
    let _ = gateway
        .close(Some(tungstenite::protocol::CloseFrame {
            code: CloseCode::Normal,
            reason: "".into(),
        }))
        .await;
    if code != 1000 {
        let _ = client
            .send(Message::Close(Some(CloseFrame {
                code,
                reason: reason.into(),
            })))
            .await;
    }
    if code != 1000 {
        log(display_id, reason, code);
    }
}

/// Only an abnormal end is a diagnostic; a finished command is not.
fn log(display_id: &str, kind: &str, code: u16) {
    eprintln!(
        "{}",
        json!({"component":"browser_relay","kind":format!("relay.{kind}"),"display_id":display_id,"code":code})
    );
}
