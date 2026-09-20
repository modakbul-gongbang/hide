//! Typed transport for Herdr's newline-delimited JSON socket API.

use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

/// The canonical schema copied from the pinned Herdr binary.
pub const HERDR_API_SCHEMA_JSON: &str = include_str!("../../contracts/herdr-api.schema.json");

include!(concat!(env!("OUT_DIR"), "/herdr_contract.rs"));

/// Herdr API protocol revision this client speaks. A mismatch is a hard,
/// explicit failure instead of a partially working sidebar.
pub const HERDR_PROTOCOL_REVISION: u64 = HERDR_PROTOCOL_REVISION_SCHEMA as u64;

#[allow(
    dead_code,
    clippy::derivable_impls,
    clippy::enum_variant_names,
    clippy::large_enum_variant
)]
pub mod wire {
    pub mod request {
        include!(concat!(env!("OUT_DIR"), "/herdr_request.rs"));
    }
    pub mod success_response {
        include!(concat!(env!("OUT_DIR"), "/herdr_success_response.rs"));
    }
    pub mod event {
        include!(concat!(env!("OUT_DIR"), "/herdr_event.rs"));
    }
    pub mod subscription_event {
        include!(concat!(env!("OUT_DIR"), "/herdr_subscription_event.rs"));
    }
    pub mod error_response {
        include!(concat!(env!("OUT_DIR"), "/herdr_error_response.rs"));
    }
}

const API_TIMEOUT: Duration = Duration::from_secs(5);

pub trait ConnectionShutdown: Send {
    fn shutdown(&self);
}

pub trait ApiStream: Read + Write + Send {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), ApiError>;

    fn set_write_timeout(&self, timeout: Option<Duration>) -> Result<(), ApiError>;

    fn read_line_with_timeout(&mut self, timeout: Duration) -> Result<String, ApiError>;

    fn shutdown_handle(&self) -> Result<Box<dyn ConnectionShutdown>, ApiError>;
}

pub trait ApiConnector: Send + Sync {
    fn connect(&self) -> Result<Box<dyn ApiStream>, ApiError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnixSocketConnector {
    socket_path: PathBuf,
}

impl UnixSocketConnector {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }
}

impl ApiConnector for UnixSocketConnector {
    fn connect(&self) -> Result<Box<dyn ApiStream>, ApiError> {
        let stream = UnixStream::connect(&self.socket_path).map_err(|error| {
            ApiError::Transport(format!(
                "connect failed for {}: {error}",
                self.socket_path.display()
            ))
        })?;
        Ok(Box::new(stream))
    }
}

struct UnixStreamShutdown(UnixStream);

impl ConnectionShutdown for UnixStreamShutdown {
    fn shutdown(&self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

impl ApiStream for UnixStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), ApiError> {
        UnixStream::set_read_timeout(self, timeout)
            .map_err(|error| ApiError::Transport(format!("read timeout could not be set: {error}")))
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> Result<(), ApiError> {
        UnixStream::set_write_timeout(self, timeout).map_err(|error| {
            ApiError::Transport(format!("write timeout could not be set: {error}"))
        })
    }

    fn read_line_with_timeout(&mut self, timeout: Duration) -> Result<String, ApiError> {
        self.set_nonblocking(true).map_err(|error| {
            ApiError::Transport(format!(
                "subscription could not enter nonblocking mode: {error}"
            ))
        })?;
        let deadline = std::time::Instant::now() + timeout;
        let mut bytes = Vec::new();
        loop {
            let mut byte = [0_u8; 1];
            match self.read(&mut byte) {
                Ok(0) => {
                    return Err(ApiError::Transport(
                        "subscription acknowledgement reached EOF".to_owned(),
                    ));
                }
                Ok(_) => {
                    bytes.push(byte[0]);
                    if byte[0] == b'\n' {
                        break;
                    }
                    if bytes.len() > 64 * 1024 {
                        return Err(ApiError::Malformed(
                            "subscription acknowledgement exceeds 64 KiB".to_owned(),
                        ));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        return Err(ApiError::Transport(
                            "subscription acknowledgement timed out".to_owned(),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => {
                    return Err(ApiError::Transport(format!(
                        "subscription acknowledgement could not be read: {error}"
                    )));
                }
            }
        }
        self.set_nonblocking(false).map_err(|error| {
            ApiError::Transport(format!(
                "subscription could not enter blocking mode: {error}"
            ))
        })?;
        String::from_utf8(bytes).map_err(|error| {
            ApiError::Malformed(format!(
                "subscription acknowledgement was not UTF-8: {error}"
            ))
        })
    }

    fn shutdown_handle(&self) -> Result<Box<dyn ConnectionShutdown>, ApiError> {
        self.try_clone()
            .map(|stream| Box::new(UnixStreamShutdown(stream)) as Box<dyn ConnectionShutdown>)
            .map_err(|error| {
                ApiError::Transport(format!(
                    "subscription shutdown handle could not be cloned: {error}"
                ))
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApiError {
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

/// The acknowledgement of `events.subscribe`. Herdr's stable release carries
/// nothing in it beyond its type: there is no event journal to resume from,
/// so a subscription always starts at "now" and a consumer that needs the
/// state before that reads a snapshot after subscribing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SubscriptionStarted {
    #[serde(rename = "type")]
    pub kind: String,
}

pub struct Subscription {
    pub ack: SubscriptionStarted,
    reader: BufReader<Box<dyn ApiStream>>,
    shutdown: Box<dyn ConnectionShutdown>,
}

impl Subscription {
    pub fn into_parts(self) -> (Box<dyn BufRead + Send>, Box<dyn ConnectionShutdown>) {
        (Box::new(self.reader), self.shutdown)
    }
}

pub fn request(socket_path: &Path, method: &str, params: Value) -> Result<Value, String> {
    request_with_timeout(socket_path, method, params, API_TIMEOUT)
        .map_err(|error| format!("{method} failed: {error}"))
}

pub fn request_with_timeout(
    socket_path: &Path,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, ApiError> {
    request_with_connector(
        &UnixSocketConnector::new(socket_path),
        method,
        params,
        timeout,
    )
}

pub fn request_with_connector(
    connector: &dyn ApiConnector,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, ApiError> {
    request_with_correlation_id(
        connector,
        &format!("herdr-core:{method}"),
        method,
        params,
        timeout,
    )
}

/// Sends one request with a caller-selected correlation id.
///
/// The pinned Herdr contract explicitly keeps this envelope id separate from
/// mutation idempotency. Callers may use it to correlate one reopen stage in
/// diagnostics, but must reconcile an ambiguous transport failure before
/// submitting that external effect again.
pub fn request_with_correlation_id(
    connector: &dyn ApiConnector,
    request_id: &str,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, ApiError> {
    let mut stream = connector.connect()?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    write_request(stream.as_mut(), request_id, method, params)?;
    let response = read_response(&mut BufReader::new(stream))?;
    response_result(response, request_id)
}

pub fn subscribe(
    socket_path: &Path,
    subscriptions: &[&str],
    timeout: Duration,
) -> Result<Subscription, ApiError> {
    subscribe_with_connector(
        &UnixSocketConnector::new(socket_path),
        subscriptions,
        timeout,
    )
}

pub fn subscribe_with_connector(
    connector: &dyn ApiConnector,
    subscriptions: &[&str],
    timeout: Duration,
) -> Result<Subscription, ApiError> {
    subscribe_with_connector_for_panes(connector, subscriptions, &[], timeout)
}

/// Open an event subscription whose filters may include the pane-scoped
/// `pane.agent_status_changed` kind.  The Herdr contract requires a
/// `pane_id` for that filter, so callers pass the pane ids discovered from
/// their bootstrap snapshot.  The other subscription kinds remain
/// unparameterized.
pub fn subscribe_with_connector_for_panes(
    connector: &dyn ApiConnector,
    subscriptions: &[&str],
    pane_ids: &[String],
    timeout: Duration,
) -> Result<Subscription, ApiError> {
    let mut stream = connector.connect()?;
    stream.set_write_timeout(Some(timeout))?;
    let request_id = "herdr-core:events.subscribe";
    write_request(
        stream.as_mut(),
        request_id,
        "events.subscribe",
        subscription_params_for_panes(subscriptions, pane_ids).map_err(ApiError::Malformed)?,
    )?;

    let response = decode_response(&stream.read_line_with_timeout(timeout)?)?;
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
    let shutdown = stream.shutdown_handle()?;
    let reader = BufReader::new(stream);
    Ok(Subscription {
        ack,
        reader,
        shutdown,
    })
}

/// Encode a contract-checked `events.subscribe` parameter object.
pub fn subscription_params(subscriptions: &[&str]) -> Result<Value, String> {
    encode_subscription_params(subscriptions, &[], false)
}

/// Encode a contract-checked `events.subscribe` parameter object, expanding
/// pane-scoped status filters once per known pane.
pub fn subscription_params_for_panes(
    subscriptions: &[&str],
    pane_ids: &[String],
) -> Result<Value, String> {
    encode_subscription_params(subscriptions, pane_ids, true)
}

fn encode_subscription_params(
    subscriptions: &[&str],
    pane_ids: &[String],
    skip_empty_status_filter: bool,
) -> Result<Value, String> {
    let mut encoded = Vec::new();
    for kind in subscriptions {
        if *kind == "pane.agent_status_changed" {
            if pane_ids.is_empty() {
                if skip_empty_status_filter {
                    continue;
                }
                return Err(
                    "invalid event subscription: pane.agent_status_changed requires pane ids"
                        .to_owned(),
                );
            }
            for pane_id in pane_ids {
                encoded.push(
                    serde_json::from_value::<wire::request::Subscription>(json!({
                        "type": kind,
                        "pane_id": pane_id,
                    }))
                    .map_err(|error| format!("invalid event subscription: {error}"))?,
                );
            }
        } else {
            encoded.push(
                serde_json::from_value::<wire::request::Subscription>(json!({"type": kind}))
                    .map_err(|error| format!("invalid event subscription: {error}"))?,
            );
        }
    }
    serde_json::to_value(wire::request::EventsSubscribeParams {
        subscriptions: encoded,
    })
    .map_err(|error| format!("subscription parameters could not be encoded: {error}"))
}

fn write_request(
    stream: &mut dyn ApiStream,
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
    stream
        .write_all(&request_line)
        .map_err(|error| ApiError::Transport(format!("request could not be written: {error}")))
}

fn read_response(reader: &mut dyn BufRead) -> Result<ResponseEnvelope, ApiError> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|error| ApiError::Transport(format!("response could not be read: {error}")))?;
    if line.trim().is_empty() {
        return Err(ApiError::Transport("response was empty".to_owned()));
    }
    decode_response(&line)
}

fn decode_response(line: &str) -> Result<ResponseEnvelope, ApiError> {
    serde_json::from_str(line)
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
    use std::io::Cursor;
    use std::os::unix::net::UnixListener;
    use std::sync::{Arc, Mutex};

    use super::*;

    struct FixtureShutdown;

    impl ConnectionShutdown for FixtureShutdown {
        fn shutdown(&self) {}
    }

    struct FixtureStream {
        incoming: Cursor<Vec<u8>>,
        outgoing: Arc<Mutex<Vec<u8>>>,
    }

    impl Read for FixtureStream {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.incoming.read(buffer)
        }
    }

    impl Write for FixtureStream {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.outgoing.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl ApiStream for FixtureStream {
        fn set_read_timeout(&self, _timeout: Option<Duration>) -> Result<(), ApiError> {
            Ok(())
        }

        fn set_write_timeout(&self, _timeout: Option<Duration>) -> Result<(), ApiError> {
            Ok(())
        }

        fn read_line_with_timeout(&mut self, _timeout: Duration) -> Result<String, ApiError> {
            let mut line = String::new();
            BufReader::new(self)
                .read_line(&mut line)
                .map_err(|error| ApiError::Transport(error.to_string()))?;
            Ok(line)
        }

        fn shutdown_handle(&self) -> Result<Box<dyn ConnectionShutdown>, ApiError> {
            Ok(Box::new(FixtureShutdown))
        }
    }

    struct FixtureConnector {
        incoming: Vec<u8>,
        outgoing: Arc<Mutex<Vec<u8>>>,
    }

    impl ApiConnector for FixtureConnector {
        fn connect(&self) -> Result<Box<dyn ApiStream>, ApiError> {
            Ok(Box::new(FixtureStream {
                incoming: Cursor::new(self.incoming.clone()),
                outgoing: Arc::clone(&self.outgoing),
            }))
        }
    }

    #[test]
    fn request_codec_accepts_a_transport_neutral_stream() {
        let outgoing = Arc::new(Mutex::new(Vec::new()));
        let connector = FixtureConnector {
            incoming: format!(
                "{}\n",
                json!({
                    "id": "herdr-core:pane.focus",
                    "result": {"type": "pane_focused", "pane_id": "w1:p2"}
                })
            )
            .into_bytes(),
            outgoing: Arc::clone(&outgoing),
        };

        let result = request_with_connector(
            &connector,
            "pane.focus",
            json!({"pane_id": "w1:p2"}),
            Duration::from_secs(1),
        )
        .expect("request succeeds over fixture transport");

        assert_eq!(result["pane_id"], "w1:p2");
        let written = String::from_utf8(outgoing.lock().unwrap().clone()).unwrap();
        let request: Value = serde_json::from_str(written.trim()).unwrap();
        assert_eq!(request["method"], "pane.focus");
        assert_eq!(request["params"], json!({"pane_id": "w1:p2"}));
    }

    #[test]
    fn request_rejects_a_response_for_another_request() {
        let root =
            Path::new("/tmp").join(format!("herdr-core-api-id-contract-{}", std::process::id()));
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

    #[test]
    fn subscribe_keeps_replay_buffered_after_the_acknowledgement() {
        let root = Path::new("/tmp").join(format!(
            "herdr-core-api-subscribe-contract-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create socket directory");
        let socket_path = root.join("herdr.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind fake socket");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request_line = String::new();
            BufReader::new(stream.try_clone().expect("clone stream"))
                .read_line(&mut request_line)
                .expect("read request");
            let request: Value = serde_json::from_str(&request_line).expect("request JSON");
            assert_eq!(request["method"], "events.subscribe");
            assert_eq!(request["params"].get("after_sequence"), None);
            assert_eq!(
                request["params"]["subscriptions"],
                json!([{"type": "pane.focused"}])
            );
            writeln!(
                stream,
                "{}",
                json!({
                    "id": "herdr-core:events.subscribe",
                    "result": {
                        "type": "subscription_started"
                    }
                })
            )
            .expect("write ack");
            writeln!(
                stream,
                "{}",
                json!({
                    "event": "pane_focused",
                    "data": {
                        "type": "pane_focused",
                        "workspace_id": "w1",
                        "pane_id": "w1:p1"
                    }
                })
            )
            .expect("write replay");
        });

        let subscription =
            subscribe(&socket_path, &["pane.focused"], Duration::from_secs(1)).expect("subscribe");
        assert_eq!(subscription.ack.kind, "subscription_started");
        let (mut reader, shutdown) = subscription.into_parts();
        let mut replay = String::new();
        reader.read_line(&mut replay).expect("read replay");
        let replay: Value = serde_json::from_str(&replay).expect("replay JSON");
        assert_eq!(replay["event"], "pane_focused");
        drop(shutdown);

        server.join().expect("fake server joins");
        std::fs::remove_file(&socket_path).expect("remove socket");
        std::fs::remove_dir(&root).expect("remove socket directory");
    }

    #[test]
    fn parameterized_status_subscriptions_are_expanded_for_known_panes() {
        let params = subscription_params_for_panes(
            &["pane.focused", "pane.agent_status_changed"],
            &["w1:p1".to_owned(), "w1:p2".to_owned()],
        )
        .expect("contract-valid subscription filters");
        assert_eq!(
            params,
            json!({
                "subscriptions": [
                    {"type": "pane.focused"},
                    {"type": "pane.agent_status_changed", "pane_id": "w1:p1"},
                    {"type": "pane.agent_status_changed", "pane_id": "w1:p2"}
                ]
            })
        );
        assert!(subscription_params(&["pane.agent_status_changed"]).is_err());
    }
}
