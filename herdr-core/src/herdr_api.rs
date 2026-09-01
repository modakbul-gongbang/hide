//! Typed transport for Herdr's local newline-delimited JSON socket API.

use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Herdr API protocol revision this core speaks. A mismatch is a hard,
/// explicit failure instead of a partially working sidebar.
pub const HERDR_PROTOCOL_REVISION: u64 = crate::herdr_contract::HERDR_PROTOCOL_REVISION as u64;

const API_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct HostScope {
    pub host_id: String,
    pub session_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ApiError {
    Transport(String),
    Remote { code: String, message: String },
    Malformed(String),
}

impl ApiError {
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Remote { code, .. } => Some(code),
            Self::Transport(_) | Self::Malformed(_) => None,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Transport(message) | Self::Malformed(message) => message,
            Self::Remote { message, .. } => message,
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(message) | Self::Malformed(message) => formatter.write_str(message),
            Self::Remote { code, message } => write!(formatter, "{code}: {message}"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct ResponseEnvelope {
    id: String,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<ApiErrorBody>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct SubscriptionStarted {
    #[serde(rename = "type")]
    kind: String,
    pub host: HostScope,
    pub sequence: u64,
    pub oldest_available_sequence: u64,
}

pub(crate) struct Subscription {
    pub ack: SubscriptionStarted,
    reader: BufReader<UnixStream>,
    shutdown: UnixStream,
}

impl Subscription {
    pub fn into_parts(self) -> (BufReader<UnixStream>, UnixStream) {
        (self.reader, self.shutdown)
    }
}

pub(crate) fn request(socket_path: &Path, method: &str, params: Value) -> Result<Value, String> {
    request_with_timeout(socket_path, method, params, API_TIMEOUT)
        .map_err(|error| format!("{method} failed: {error}"))
}

pub(crate) fn request_with_timeout(
    socket_path: &Path,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, ApiError> {
    let mut stream = connect(socket_path, timeout)?;
    let request_id = format!("herdr-core:{method}");
    write_request(&mut stream, &request_id, method, params)?;
    let response = read_response(&mut BufReader::new(stream))?;
    response_result(response, &request_id)
}

pub(crate) fn subscribe(
    socket_path: &Path,
    after_sequence: u64,
    subscriptions: &[&str],
    timeout: Duration,
) -> Result<Subscription, ApiError> {
    let mut stream = connect(socket_path, timeout)?;
    let request_id = "herdr-core:events.subscribe";
    let filters = subscriptions
        .iter()
        .map(|event_type| json!({"type": event_type}))
        .collect::<Vec<_>>();
    write_request(
        &mut stream,
        request_id,
        "events.subscribe",
        json!({
            "after_sequence": after_sequence,
            "subscriptions": filters,
        }),
    )?;

    let mut reader = BufReader::new(stream);
    let response = read_response(&mut reader)?;
    let result = response_result(response, request_id)?;
    let ack: SubscriptionStarted = serde_json::from_value(result).map_err(|error| {
        ApiError::Malformed(format!(
            "events.subscribe acknowledgement is malformed: {error}"
        ))
    })?;
    if ack.kind != "subscription_started" {
        return Err(ApiError::Malformed(format!(
            "events.subscribe returned unexpected result type {:?}",
            ack.kind
        )));
    }
    reader
        .get_ref()
        .set_read_timeout(None)
        .map_err(|error| {
            ApiError::Transport(format!(
                "subscription read timeout could not be cleared: {error}"
            ))
        })?;
    let shutdown = reader.get_ref().try_clone().map_err(|error| {
        ApiError::Transport(format!(
            "subscription shutdown handle could not be cloned: {error}"
        ))
    })?;
    Ok(Subscription {
        ack,
        reader,
        shutdown,
    })
}

fn connect(socket_path: &Path, timeout: Duration) -> Result<UnixStream, ApiError> {
    let stream = UnixStream::connect(socket_path).map_err(|error| {
        ApiError::Transport(format!(
            "connect failed for {}: {error}",
            socket_path.display()
        ))
    })?;
    stream.set_read_timeout(Some(timeout)).map_err(|error| {
        ApiError::Transport(format!("read timeout could not be set: {error}"))
    })?;
    stream.set_write_timeout(Some(timeout)).map_err(|error| {
        ApiError::Transport(format!("write timeout could not be set: {error}"))
    })?;
    Ok(stream)
}

fn write_request(
    stream: &mut UnixStream,
    request_id: &str,
    method: &str,
    params: Value,
) -> Result<(), ApiError> {
    let envelope = json!({
        "id": request_id,
        "method": method,
        "params": params,
    });
    let mut request_line = serde_json::to_vec(&envelope)
        .map_err(|error| ApiError::Malformed(format!("request could not be encoded: {error}")))?;
    request_line.push(b'\n');
    stream.write_all(&request_line).map_err(|error| {
        ApiError::Transport(format!("request could not be written: {error}"))
    })
}

fn read_response(reader: &mut BufReader<UnixStream>) -> Result<ResponseEnvelope, ApiError> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|error| ApiError::Transport(format!("response could not be read: {error}")))?;
    if line.trim().is_empty() {
        return Err(ApiError::Transport("response was empty".to_owned()));
    }
    serde_json::from_str(&line)
        .map_err(|error| ApiError::Malformed(format!("response was not valid JSON: {error}")))
}

fn response_result(response: ResponseEnvelope, request_id: &str) -> Result<Value, ApiError> {
    if response.id != request_id {
        return Err(ApiError::Malformed(format!(
            "response id {:?} does not match request id {request_id:?}",
            response.id
        )));
    }
    match (response.result, response.error) {
        (Some(result), None) => Ok(result),
        (None, Some(error)) => Err(ApiError::Remote {
            code: error.code,
            message: error.message,
        }),
        (Some(_), Some(_)) => Err(ApiError::Malformed(
            "response contains both result and error".to_owned(),
        )),
        (None, None) => Err(ApiError::Malformed(
            "response contains neither result nor error".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;

    use super::*;

    #[test]
    fn request_rejects_a_response_for_another_request() {
        let root = Path::new("/tmp").join(format!(
            "herdr-core-api-id-contract-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create socket directory");
        let socket_path = root.join("herdr.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind fake socket");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut ignored = String::new();
            BufReader::new(stream.try_clone().expect("clone stream"))
                .read_line(&mut ignored)
                .expect("read request");
            writeln!(stream, "{}", json!({"id": "somebody-else", "result": {}}))
                .expect("write response");
        });

        let error = request_with_timeout(
            &socket_path,
            "session.snapshot",
            json!({}),
            Duration::from_secs(1),
        )
        .expect_err("mismatched id must fail");
        assert!(matches!(error, ApiError::Malformed(_)));

        server.join().expect("fake server joins");
        std::fs::remove_file(&socket_path).expect("remove socket");
        std::fs::remove_dir(&root).expect("remove socket directory");
    }
}
