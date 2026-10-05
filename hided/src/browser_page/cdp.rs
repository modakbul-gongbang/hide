//! A small CDP client over the relay socket: request ids, flattened session
//! ids, and the events that arrive between replies. One command owns one
//! client, and dropping it closes the socket, which ends the gateway lease.

use std::collections::VecDeque;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;

use crate::workspace_cli::WorkspaceSocket;

/// Events kept between replies. A console replay is at most V8's 1000 stored
/// messages, so this never drops one a command reads.
const MAX_EVENTS: usize = 4096;

#[derive(Debug)]
pub enum CdpError {
    /// The page answered with a protocol error.
    Protocol(String),
    /// No answer before the client's deadline.
    Timeout,
    /// The relay or the gateway closed the connection.
    Closed { code: u16, reason: String },
}

#[derive(Debug, Clone)]
pub struct Event {
    pub method: String,
    pub params: Value,
    /// The flattened session the event belongs to; none for the browser.
    pub session: Option<String>,
}

pub struct Cdp {
    socket: WorkspaceSocket,
    next_id: u64,
    events: VecDeque<Event>,
    /// A JavaScript dialog the page opened while this client listened.
    pub dialog: Option<Value>,
}

impl Cdp {
    pub fn new(socket: WorkspaceSocket) -> Self {
        Self {
            socket,
            next_id: 0,
            events: VecDeque::new(),
            dialog: None,
        }
    }

    pub async fn call(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
        timeout: Duration,
    ) -> Result<Value, CdpError> {
        self.call_inner(method, params, session, timeout, false)
            .await
            .map(|answer| answer.unwrap_or(Value::Null))
    }

    /// An input event whose handler opens a dialog is acknowledged only after
    /// the operator answers it, so waiting stops when the dialog opens.
    /// `None` means the dialog interrupted the wait.
    pub async fn call_input(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
        timeout: Duration,
    ) -> Result<Option<Value>, CdpError> {
        if self.dialog.is_some() {
            return Ok(None);
        }
        self.call_inner(method, params, session, timeout, true)
            .await
    }

    async fn call_inner(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
        timeout: Duration,
        stop_on_dialog: bool,
    ) -> Result<Option<Value>, CdpError> {
        self.next_id += 1;
        let id = self.next_id;
        let mut message = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        self.socket
            .send(Message::Text(message.to_string().into()))
            .await
            .map_err(|_| self.closed_error())?;
        let deadline = Instant::now() + timeout;
        loop {
            let frame = tokio::time::timeout_at(deadline, self.socket.next())
                .await
                .map_err(|_| CdpError::Timeout)?;
            let text = match frame {
                Some(Ok(Message::Text(text))) => text,
                Some(Ok(Message::Close(frame))) => {
                    return Err(match frame {
                        Some(frame) => CdpError::Closed {
                            code: u16::from(frame.code),
                            reason: frame.reason.to_string(),
                        },
                        None => CdpError::Closed {
                            code: 1005,
                            reason: String::new(),
                        },
                    });
                }
                Some(Ok(_)) => continue,
                Some(Err(_)) | None => return Err(self.closed_error()),
            };
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if value["id"].as_u64() == Some(id) {
                if let Some(error) = value.get("error") {
                    return Err(CdpError::Protocol(
                        error["message"].as_str().unwrap_or("").to_owned(),
                    ));
                }
                return Ok(Some(value.get("result").cloned().unwrap_or(Value::Null)));
            }
            if let Some(method) = value["method"].as_str() {
                let event = Event {
                    method: method.to_owned(),
                    params: value.get("params").cloned().unwrap_or(Value::Null),
                    session: value["sessionId"].as_str().map(str::to_owned),
                };
                if event.method == "Page.javascriptDialogOpening" {
                    self.dialog = Some(json!({
                        "type": event.params["type"],
                        "message": event.params["message"].as_str().unwrap_or("").chars().take(300).collect::<String>(),
                    }));
                    if stop_on_dialog {
                        self.push(event);
                        return Ok(None);
                    }
                }
                self.push(event);
            }
        }
    }

    fn push(&mut self, event: Event) {
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    fn closed_error(&self) -> CdpError {
        CdpError::Closed {
            code: 1006,
            reason: String::new(),
        }
    }

    /// Takes the buffered events that match, leaving the rest in order.
    pub fn take_events(&mut self, keep: impl Fn(&Event) -> bool) -> Vec<Event> {
        let mut taken = Vec::new();
        self.events.retain(|event| {
            if keep(event) {
                taken.push(event.clone());
                false
            } else {
                true
            }
        });
        taken
    }

    pub async fn close(mut self) {
        let _ = tokio::time::timeout(Duration::from_secs(1), self.socket.close(None)).await;
    }
}
