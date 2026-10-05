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
/// No command runs this long (`wait` is at most a minute), so a relay that
/// does is released whatever it carries.
const LIFETIME: Duration = Duration::from_secs(300);
/// A peer that does not take a frame within this has stopped reading.
const SEND: Duration = Duration::from_secs(5);
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
/// cap, no CDP text moves for the idle bound, or the relay reaches its
/// lifetime; a peer that stops reading is dropped after a bounded send.
/// Every exit closes both sides.
pub async fn pump(client: &mut WebSocket, mut gateway: Gateway, display_id: &str) {
    let end = tokio::time::Instant::now() + LIFETIME;
    let mut deadline = tokio::time::Instant::now() + IDLE;
    let (code, reason) = loop {
        tokio::select! {
            incoming = client.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if text.len() > MAX_MESSAGE_BYTES {
                        break (CLOSE_MESSAGE_LIMIT, "browser_relay_message_limit");
                    }
                    let sent = tokio::time::timeout(SEND, gateway.send(GatewayMessage::Text(text.as_str().into()))).await;
                    if !matches!(sent, Ok(Ok(()))) {
                        break (CLOSE_GATEWAY_LOST, "browser_relay_gateway_lost");
                    }
                }
                Some(Ok(Message::Binary(_))) => break (CLOSE_MESSAGE_LIMIT, "browser_relay_text_only"),
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break (1000, "client_closed"),
                // Pings keep nothing alive: only CDP traffic resets the idle bound.
                Some(Ok(_)) => continue,
            },
            outgoing = gateway.next() => match outgoing {
                Some(Ok(GatewayMessage::Text(text))) => {
                    let sent = tokio::time::timeout(SEND, client.send(Message::Text(text.as_str().into()))).await;
                    if !matches!(sent, Ok(Ok(()))) {
                        break (1000, "client_closed");
                    }
                }
                Some(Ok(GatewayMessage::Close(frame))) => {
                    // The gateway's own reason (a closed display, a resource
                    // limit) reaches the CLI unchanged.
                    let (code, text) = frame.map_or((1000, String::new()), |frame| (u16::from(frame.code), frame.reason.to_string()));
                    let _ = tokio::time::timeout(SEND, client.send(Message::Close(Some(CloseFrame { code, reason: text.into() })))).await;
                    if code != 1000 {
                        log(display_id, "gateway_closed", code);
                    }
                    return;
                }
                Some(Err(tungstenite::Error::Capacity(_))) => break (CLOSE_MESSAGE_LIMIT, "browser_relay_message_limit"),
                Some(Ok(_)) => continue,
                None | Some(Err(_)) => break (CLOSE_GATEWAY_LOST, "browser_relay_gateway_lost"),
            },
            () = tokio::time::sleep_until(deadline.min(end)) => break (CLOSE_IDLE, if deadline < end { "browser_relay_idle" } else { "browser_relay_lifetime" }),
        }
        deadline = tokio::time::Instant::now() + IDLE;
    };
    let _ = tokio::time::timeout(
        SEND,
        gateway.close(Some(tungstenite::protocol::CloseFrame {
            code: CloseCode::Normal,
            reason: "".into(),
        })),
    )
    .await;
    if code != 1000 {
        let _ = tokio::time::timeout(
            SEND,
            client.send(Message::Close(Some(CloseFrame {
                code,
                reason: reason.into(),
            }))),
        )
        .await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::ws::WebSocketUpgrade;
    use axum::routing::get;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

    /// A gateway that echoes each short text frame, as a CDP endpoint
    /// answers, and drops anything else, so only the relay's own checks can
    /// close the connection.
    async fn echo_gateway() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(Ok(message)) = socket.next().await {
                if let GatewayMessage::Text(text) = message
                    && text.len() < 64
                    && socket.send(GatewayMessage::Text(text)).await.is_err()
                {
                    break;
                }
            }
        });
        format!("ws://{address}/")
    }

    /// A daemon route that relays one client to the gateway.
    async fn relay_to(gateway: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/",
            get(move |upgrade: WebSocketUpgrade| async move {
                upgrade.on_upgrade(move |mut socket| async move {
                    let gateway = connect(&gateway).await.unwrap();
                    pump(&mut socket, gateway, "d1").await;
                })
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("ws://{address}/")
    }

    async fn close_code(message: GatewayMessage) -> u16 {
        let relay = relay_to(echo_gateway().await).await;
        let (mut client, _) = tokio_tungstenite::connect_async(relay.as_str())
            .await
            .unwrap();
        client
            .send(GatewayMessage::Text(r#"{"id":1}"#.into()))
            .await
            .unwrap();
        assert_eq!(
            client.next().await.unwrap().unwrap(),
            GatewayMessage::Text(r#"{"id":1}"#.into()),
            "a CDP frame crosses the relay both ways"
        );
        client.send(message).await.unwrap();
        loop {
            match client.next().await {
                Some(Ok(GatewayMessage::Close(Some(frame)))) => return u16::from(frame.code),
                Some(Ok(_)) => continue,
                other => panic!("the relay ended without a close frame: {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn a_frame_over_the_cap_closes_the_relay_with_its_reason() {
        let oversized = "x".repeat(MAX_MESSAGE_BYTES + 1);
        assert_eq!(
            close_code(GatewayMessage::Text(oversized.into())).await,
            CLOSE_MESSAGE_LIMIT
        );
    }

    #[tokio::test]
    async fn a_binary_frame_closes_the_relay_with_its_reason() {
        assert_eq!(
            close_code(GatewayMessage::Binary(vec![1, 2, 3].into())).await,
            CLOSE_MESSAGE_LIMIT
        );
        assert_ne!(CLOSE_MESSAGE_LIMIT, u16::from(CloseCode::Normal));
    }
}
