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

/// The events a command reads; every other event is dropped on arrival.
const KEPT: &[&str] = &[
    "Target.attachedToTarget",
    "Target.detachedFromTarget",
    "Input.dragIntercepted",
    "Runtime.consoleAPICalled",
    "Runtime.exceptionThrown",
];
/// Events kept between replies. A console replay is at most V8's 1000 stored
/// messages, so the count never drops one a command reads; the byte bound
/// keeps a page that logs large values from growing the client.
const MAX_EVENTS: usize = 4096;
const MAX_EVENT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub enum CdpError {
    /// The page answered with a protocol error.
    Protocol(String),
    /// No answer before the client's deadline.
    Timeout,
    /// The relay or the gateway closed the connection.
    Closed { code: u16, reason: String },
}

/// What one answer message says: its result, or the page's protocol error.
fn answer(value: &Value) -> Result<Value, CdpError> {
    match value.get("error") {
        Some(error) => Err(CdpError::Protocol(
            error["message"].as_str().unwrap_or("").to_owned(),
        )),
        None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
    }
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
    events: VecDeque<(Event, usize)>,
    event_bytes: usize,
    /// A JavaScript dialog the page opened while this client listened.
    pub dialog: Option<Value>,
}

impl Cdp {
    pub fn new(socket: WorkspaceSocket) -> Self {
        Self {
            socket,
            next_id: 0,
            events: VecDeque::new(),
            event_bytes: 0,
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
        let id = self.send(method, params, session).await?;
        let deadline = Instant::now() + timeout;
        loop {
            let (value, bytes) = self.next_message(deadline).await?;
            if value["id"].as_u64() == Some(id) {
                return Ok(Some(answer(&value)?));
            }
            if self.note(&value, bytes) && stop_on_dialog {
                return Ok(None);
            }
        }
    }

    /// Sends every call before reading any answer and reads them under one
    /// deadline, so calls the page holds cost one timeout between them, not
    /// one each. A call with no answer by the deadline is `Timeout`; the
    /// connection ending is the error of the whole batch.
    pub async fn call_all(
        &mut self,
        calls: &[(&str, Value, &str)],
        timeout: Duration,
    ) -> Result<Vec<Result<Value, CdpError>>, CdpError> {
        let mut ids = Vec::with_capacity(calls.len());
        for (method, params, session) in calls {
            ids.push(self.send(method, params.clone(), Some(session)).await?);
        }
        let mut answers: Vec<Option<Result<Value, CdpError>>> = ids.iter().map(|_| None).collect();
        let deadline = Instant::now() + timeout;
        while answers.iter().any(Option::is_none) {
            let (value, bytes) = match self.next_message(deadline).await {
                Ok(message) => message,
                Err(CdpError::Timeout) => break,
                Err(error) => return Err(error),
            };
            match value["id"]
                .as_u64()
                .and_then(|id| ids.iter().position(|sent| *sent == id))
            {
                Some(index) => answers[index] = Some(answer(&value)),
                None => {
                    self.note(&value, bytes);
                }
            }
        }
        Ok(answers
            .into_iter()
            .map(|answer| answer.unwrap_or(Err(CdpError::Timeout)))
            .collect())
    }

    async fn send(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
    ) -> Result<u64, CdpError> {
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
        Ok(id)
    }

    /// The next JSON message the socket carries, with its size, or `Timeout`
    /// at the deadline.
    async fn next_message(&mut self, deadline: Instant) -> Result<(Value, usize), CdpError> {
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
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                return Ok((value, text.len()));
            }
        }
    }

    /// Keeps the events a command reads; true when the message was a
    /// JavaScript dialog opening.
    fn note(&mut self, value: &Value, bytes: usize) -> bool {
        let Some(method) = value["method"].as_str() else {
            return false;
        };
        if method != "Page.javascriptDialogOpening" && !KEPT.contains(&method) {
            return false;
        }
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
            return true;
        }
        self.push(event, bytes);
        false
    }

    fn push(&mut self, event: Event, bytes: usize) {
        self.events.push_back((event, bytes));
        self.event_bytes += bytes;
        while self.events.len() > MAX_EVENTS || self.event_bytes > MAX_EVENT_BYTES {
            let Some((_, dropped)) = self.events.pop_front() else {
                break;
            };
            self.event_bytes -= dropped;
        }
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
        let mut freed = 0;
        self.events.retain(|(event, bytes)| {
            if keep(event) {
                taken.push(event.clone());
                freed += bytes;
                false
            } else {
                true
            }
        });
        self.event_bytes -= freed;
        taken
    }

    pub async fn close(mut self) {
        let _ = tokio::time::timeout(Duration::from_secs(1), self.socket.close(None)).await;
    }
}
