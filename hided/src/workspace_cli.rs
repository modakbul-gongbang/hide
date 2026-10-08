//! Pane-scoped Workspace CLI transport. Bootstrap attests the caller over a
//! local socket; commands and results use hided's authenticated `/ws`.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use herdr_core::workspace_control::Action;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::ORIGIN;

use hide_platform::fs::private;
use hide_platform::ipc::LocalStream;

use crate::env::Env;
use crate::pane_auth::{Reference, Route};
use crate::state_file::SCHEMA_VERSION;

const TIMEOUT: Duration = Duration::from_secs(10);
/// Longer than the daemon's own Factory answer limit, so the daemon's reason
/// arrives rather than a local timeout.
const FACTORY_TIMEOUT: Duration = Duration::from_secs(110);
const REMOTE_BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(25);

/// An auto-bootstrapped direct CLI owns its reference even if transport fails.
struct OneShotReference(PathBuf);

impl Drop for OneShotReference {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        let _ = fs::remove_file(self.0.with_extension("claimed"));
    }
}

/// The credential one command runs with: the reference `HIDE_CAP_REF` names,
/// or a one-shot reference the command bootstrapped and removes when it ends.
///
/// A named reference outlives nothing it was issued for: when it has expired
/// or the daemon revoked it, the command bootstraps a one-shot credential
/// exactly as a bare `hide` does, once, so the fallback passes the same
/// caller attestation and carries no authority a bare command would not get.
pub struct Credential<'a> {
    env: &'a Env,
    path: PathBuf,
    /// The reference came from `HIDE_CAP_REF` and has not been replaced.
    named: bool,
    _one_shot: Option<OneShotReference>,
}

impl<'a> Credential<'a> {
    pub fn acquire(env: &'a Env) -> Result<Self, String> {
        match std::env::var(crate::env::HIDE_CAP_REF) {
            Ok(value) if !value.is_empty() => Ok(Self::named(env, PathBuf::from(value))),
            Ok(_) => Err("invalid_reference".to_owned()),
            Err(_) => Self::one_shot(env),
        }
    }

    /// The credential a `HIDE_CAP_REF` naming `path` gives.
    pub fn named(env: &'a Env, path: PathBuf) -> Self {
        Self {
            env,
            path,
            named: true,
            _one_shot: None,
        }
    }

    fn one_shot(env: &'a Env) -> Result<Self, String> {
        let path = bootstrap(env, true)?;
        Ok(Self {
            env,
            path: path.clone(),
            named: false,
            _one_shot: Some(OneShotReference(path)),
        })
    }

    /// Replaces a named reference the daemon no longer holds with a bare
    /// one-shot bootstrap. False when `reason` does not say the reference is
    /// gone or the credential was already bootstrapped, so a request retries
    /// once.
    fn renew(&mut self, reason: &str) -> Result<bool, String> {
        if !self.named || !matches!(reason, "credential_expired" | UNREACHABLE) {
            return Ok(false);
        }
        *self = Self::one_shot(self.env)?;
        Ok(true)
    }

    /// Runs one request, and again with a bare bootstrap when the named
    /// reference is gone. A gone reference is found before the daemon runs
    /// the command (the file, the connect, the handshake, the daemon's
    /// credential check) or, as a lost claim acknowledgement, after a read;
    /// a lost claim after an action keeps its result, so the second run
    /// never repeats an applied action.
    fn run<T>(&mut self, mut request: impl FnMut(&Path) -> Result<T, String>) -> Result<T, String> {
        match request(&self.path) {
            Err(reason) if self.renew(&reason)? => request(&self.path),
            outcome => outcome,
        }
        .map_err(reported)
    }
}

/// No daemon listens where the reference points, before anything was sent:
/// the daemon that issued it stopped without removing it. Reported as
/// `hide_unavailable`, the reason a caller already acts on.
const UNREACHABLE: &str = "hide_unreachable";

fn reported(reason: String) -> String {
    if reason == UNREACHABLE {
        "hide_unavailable".to_owned()
    } else {
        reason
    }
}

pub fn bootstrap(env: &Env, one_shot: bool) -> Result<PathBuf, String> {
    // A caller with no pane id (a daemon first started from a plain terminal)
    // still bootstraps: hided binds it to the registered checkout holding
    // its cwd, or refuses with the reason the caller can act on.
    let pane_id = env.pane_id.as_deref().unwrap_or("");
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce).map_err(|_| "reference_unavailable".to_owned())?;
    let request = json!({"pane_id":pane_id,"nonce":hex::encode(nonce),"one_shot":one_shot});
    match bootstrap_local(env, &request) {
        Ok(path) => Ok(path),
        Err(local_reason) => match bootstrap_remote(env, &request) {
            Ok(Some(path)) => Ok(path),
            Ok(None) => Err(local_reason),
            Err(remote_reason) => Err(remote_reason),
        },
    }
}

fn bootstrap_local(env: &Env, request: &Value) -> Result<PathBuf, String> {
    let socket = crate::pane_auth::bootstrap_socket_path(&env.state_dir)?;
    let mut stream = LocalStream::connect(&socket).map_err(|_| "hide_unavailable".to_owned())?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|_| "hide_unavailable".to_owned())?;
    match stream.set_write_timeout(Some(TIMEOUT)) {
        // A Windows pipe has no write timeout; the request is one line that
        // fits the pipe's buffer, and the read that follows is bounded.
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported => {}
        result => result.map_err(|_| "hide_unavailable".to_owned())?,
    }
    writeln!(stream, "{request}").map_err(|_| "hide_unavailable".to_owned())?;
    read_bootstrap_answer(&mut stream)
}

fn read_bootstrap_answer(stream: &mut impl Read) -> Result<PathBuf, String> {
    let mut line = String::new();
    BufReader::new(stream)
        .take(4096)
        .read_line(&mut line)
        .map_err(|_| "hide_unavailable".to_owned())?;
    let answer: Value = serde_json::from_str(&line).map_err(|_| "hide_unavailable".to_owned())?;
    if answer["ok"] != true {
        return Err(answer["reason"]
            .as_str()
            .unwrap_or("bootstrap_refused")
            .to_owned());
    }
    answer["reference"]
        .as_str()
        .and_then(|wire| hide_platform::path::from_wire(wire).ok())
        .ok_or_else(|| "reference_unavailable".to_owned())
}

/// Asks each node pane service on this device (`hide_host::panes`), one per
/// daemon that has this device open, for this pane's reference.
fn bootstrap_remote(env: &Env, request: &Value) -> Result<Option<PathBuf>, String> {
    bootstrap_bridges(
        &hide_kit::layout::workspace_bridges(&env.state_dir),
        request,
    )
}

fn bootstrap_bridges(bridge_dir: &Path, request: &Value) -> Result<Option<PathBuf>, String> {
    let bridge_dir = bridge_dir.to_path_buf();
    let metadata = match fs::symlink_metadata(&bridge_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("bridge_unavailable".to_owned()),
    };
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || !private::owned_by_current_user(&bridge_dir).unwrap_or(false)
        || !private::is_private(&bridge_dir).unwrap_or(false)
    {
        return Err("bridge_unavailable".to_owned());
    }
    let entries = match fs::read_dir(&bridge_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("bridge_unavailable".to_owned()),
    };
    let mut success = None;
    let mut reason = None;
    let mut seen = 0;
    for entry in entries {
        let entry = entry.map_err(|_| "bridge_unavailable".to_owned())?;
        if !entry.file_name().to_string_lossy().starts_with("bridge-") {
            continue;
        }
        // A socket on Unix, the listener's marker file on Windows; the
        // connect tells a live listener from what a dead one left.
        let socket = entry.path().join("bootstrap.sock");
        let Ok(metadata) = fs::symlink_metadata(&socket) else {
            continue;
        };
        if metadata.is_dir()
            || metadata.file_type().is_symlink()
            || !private::owned_by_current_user(&socket).unwrap_or(false)
        {
            continue;
        }
        let Ok(mut stream) = LocalStream::connect(&socket) else {
            continue;
        };
        seen += 1;
        if seen > 16 {
            return Err("bridge_limit".to_owned());
        }
        let _ = stream.set_read_timeout(Some(REMOTE_BOOTSTRAP_TIMEOUT));
        // A Windows pipe has no write timeout; the request is one short line.
        let _ = stream.set_write_timeout(Some(REMOTE_BOOTSTRAP_TIMEOUT));
        if writeln!(stream, "{request}").is_err() {
            reason = Some("bridge_unavailable".to_owned());
            continue;
        }
        match read_bootstrap_answer(&mut stream) {
            Ok(path) if path.starts_with(&bridge_dir) => {
                if success.is_some() {
                    // A bridge may return a persistent reference already held
                    // by a live pane. Ambiguity cannot revoke either holder.
                    return Err("ambiguous_pane".to_owned());
                }
                success = Some(path);
            }
            Ok(_) => reason = Some("invalid_reference".to_owned()),
            Err(error) => reason = Some(error),
        }
    }
    if success.is_some() {
        Ok(success)
    } else if let Some(reason) = reason {
        Err(reason)
    } else {
        Ok(None)
    }
}

fn read_reference(path: &Path) -> Result<Reference, String> {
    if !path.is_absolute() {
        return Err("invalid_reference".to_owned());
    }
    // The opened file is this account's own regular file and never what a
    // link at the path leads to, so replacing the path with a link between a
    // check and the read cannot make this helper read an unrelated file.
    let file = private::open_own_file(path, false).map_err(|_| "credential_expired".to_owned())?;
    let metadata = file
        .metadata()
        .map_err(|_| "credential_expired".to_owned())?;
    if !private::is_private(path).unwrap_or(false) || metadata.len() > 4096 {
        return Err("invalid_reference".to_owned());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "credential_expired".to_owned())?;
    if bytes.len() > 4096 {
        return Err("invalid_reference".to_owned());
    }
    let reference: Reference =
        serde_json::from_slice(&bytes).map_err(|_| "invalid_reference".to_owned())?;
    if reference.token.len() != 64 || !reference.token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid_reference".to_owned());
    }
    let valid = match &reference.route {
        Route::Daemon { port, origin_port } => *port != 0 && *origin_port != 0,
        Route::Node { socket } => node_socket(socket).is_some(),
    };
    if !valid {
        return Err("invalid_reference".to_owned());
    }
    Ok(reference)
}

pub fn request(credential: &mut Credential, query: &str) -> Result<Value, String> {
    credential.run(|path| request_query(path, query, None))
}

pub fn browser_connect(
    credential: &mut Credential,
    display_id: Option<&str>,
) -> Result<Value, String> {
    credential.run(|path| request_query(path, "browser_connect", display_id))
}

fn request_query(path: &Path, query: &str, display_id: Option<&str>) -> Result<Value, String> {
    let reference = read_reference(path)?;
    let request_id = fresh_request_id()?;
    run_exchange(
        path,
        &reference,
        json!({"type":"workspace_query","request_id":request_id,"query":query,"display_id":display_id}),
        &request_id,
        false,
    )
}

/// `hide links`: a read, answered without a shell.
pub fn request_links(
    credential: &mut Credential,
    query: &herdr_core::links::query::LinksQuery,
) -> Result<Value, String> {
    credential.run(|path| {
        let reference = read_reference(path)?;
        let request_id = fresh_request_id()?;
        run_exchange(
            path,
            &reference,
            json!({"type":"links","request_id":request_id,"query":query}),
            &request_id,
            false,
        )
    })
}

/// Losing the final capability claim cannot turn durable success into retry.
pub fn request_delivery(
    credential: &mut Credential,
    command: herdr_core::delivery::Command,
    hint: Option<&str>,
) -> Result<Value, String> {
    credential.run(|path| {
        let reference = read_reference(path)?;
        let request_id = fresh_request_id()?;
        run_exchange(
            path,
            &reference,
            json!({
                "type":"delivery", "request_id":request_id, "command":&command, "caller_pane":hint
            }),
            &request_id,
            true,
        )
    })
}

/// A `hide factory` command. `add` may wait for its intake review, so the
/// answer has longer than a Workspace request to arrive.
pub fn request_factory(
    credential: &mut Credential,
    command: &hide_factory::Command,
    hint: Option<&str>,
) -> Result<Value, String> {
    credential.run(|path| {
        let reference = read_reference(path)?;
        let request_id = fresh_request_id()?;
        run_exchange_within(
            path,
            &reference,
            json!({
                "type":"factory", "request_id":request_id, "command":command, "caller_pane":hint
            }),
            &request_id,
            true,
            FACTORY_TIMEOUT,
        )
    })
}

/// One question check, with no automatic replay on an expired credential.
pub fn request_factory_question_guard(
    credential: &Credential,
    session: &str,
    agent_runtime: &str,
) -> Result<Value, String> {
    let path = &credential.path;
    let reference = read_reference(path)?;
    let request_id = fresh_request_id()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "request_unavailable".to_owned())?;
    runtime.block_on(async {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let (value, mut socket) = tokio::time::timeout_at(
            deadline,
            exchange_response_with_limit(
                &reference,
                json!({"type":"factory_question_guard", "request_id":request_id,
                "session":session, "runtime":agent_runtime}),
                &request_id,
                Some(16 * 1024),
            ),
        )
        .await
        .map_err(|_| "factory_guard_expired".to_owned())??;
        tokio::time::timeout_at(deadline, claim(&mut socket, path))
            .await
            .map_err(|_| "factory_guard_expired".to_owned())??;
        Ok(value)
    })
}

/// The relay socket for one `hide browser` page command: the same scoped
/// handshake and claim as every Workspace request, after which the socket
/// carries CDP frames to the caller's display. A refusal keeps the daemon's
/// reason and next action.
/// The relay socket, and whether the display is its area's selected View.
pub(crate) async fn browser_relay(
    credential: &mut Credential<'_>,
    display_id: &str,
) -> Result<(WorkspaceSocket, bool), (String, Option<String>)> {
    let renewed = |reason: String| {
        let next_action = crate::cli::bootstrap_next_action(&reason).to_owned();
        (reason, Some(next_action))
    };
    match relay_once(&credential.path, display_id).await {
        Err((reason, _)) if credential.renew(&reason).map_err(renewed)? => {
            relay_once(&credential.path, display_id).await
        }
        outcome => outcome,
    }
    .map_err(|(reason, next_action)| (reported(reason), next_action))
}

async fn relay_once(
    path: &Path,
    display_id: &str,
) -> Result<(WorkspaceSocket, bool), (String, Option<String>)> {
    let reference = read_reference(path).map_err(|reason| (reason, None))?;
    let request_id = fresh_request_id().map_err(|reason| (reason, None))?;
    let payload = json!({"type":"browser_relay","request_id":request_id,"display_id":display_id});
    let (answer, mut socket) =
        tokio::time::timeout(TIMEOUT, exchange_response(&reference, payload, &request_id))
            .await
            .map_err(|_| ("request_timeout".to_owned(), None))?
            .map_err(|reason| (reason, None))?;
    if answer["ok"] != true {
        return Err((
            answer["reason"]
                .as_str()
                .unwrap_or("browser_relay_refused")
                .to_owned(),
            answer["next_action"].as_str().map(str::to_owned),
        ));
    }
    tokio::time::timeout(Duration::from_secs(2), claim(&mut socket, path))
        .await
        .map_err(|_| ("credential_expired".to_owned(), None))?
        .map_err(|reason| (reason, None))?;
    Ok((socket, answer["result"]["selected"] == true))
}

pub fn request_action(
    credential: &mut Credential,
    action: Action,
    request_id: &str,
) -> Result<Value, String> {
    if !valid_request_id(request_id) {
        return Err("invalid_request_id".to_owned());
    }
    credential.run(|path| {
        let reference = read_reference(path)?;
        run_exchange(
            path,
            &reference,
            json!({"type":"workspace_action","request_id":request_id,"command":&action}),
            request_id,
            true,
        )
    })
}

pub fn valid_request_id(id: &str) -> bool {
    let Some((timestamp, suffix)) = id.split_once('-') else {
        return false;
    };
    id.len() <= 64
        && !suffix.is_empty()
        && timestamp.parse::<u64>().is_ok()
        && suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn fresh_request_id() -> Result<String, String> {
    let mut id = [0u8; 16];
    getrandom::getrandom(&mut id).map_err(|_| "request_unavailable".to_owned())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "request_unavailable".to_owned())?
        .as_millis();
    Ok(format!("{now}-{}", hex::encode(id)))
}

fn run_exchange(
    path: &Path,
    reference: &Reference,
    payload: Value,
    request_id: &str,
    action: bool,
) -> Result<Value, String> {
    run_exchange_within(path, reference, payload, request_id, action, TIMEOUT)
}

fn run_exchange_within(
    path: &Path,
    reference: &Reference,
    payload: Value,
    request_id: &str,
    action: bool,
    timeout: Duration,
) -> Result<Value, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "request_unavailable".to_owned())?;
    let (value, mut socket) = runtime.block_on(async {
        tokio::time::timeout(timeout, exchange_response(reference, payload, request_id))
            .await
            .map_err(|_| "request_timeout".to_owned())?
    })?;
    let claimed = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(2), claim(&mut socket, path))
            .await
            .map_err(|_| "credential_expired".to_owned())?
    });
    if let Err(reason) = claimed {
        eprintln!(
            "{}",
            json!({"component":"workspace_cli","kind":"claim.failed","request_id":request_id,"reason":reason})
        );
        if !action {
            return Err(reason);
        }
    }
    Ok(value)
}

/// The bytes under a Workspace socket: a loopback connection, or a stream
/// through a device's node.
pub(crate) trait Transport: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Transport for T {}

pub(crate) type WorkspaceSocket = tokio_tungstenite::WebSocketStream<Pin<Box<dyn Transport>>>;

async fn connect(reference: &Reference) -> Result<WorkspaceSocket, String> {
    connect_with_limit(reference, None).await
}

async fn connect_with_limit(
    reference: &Reference,
    response_limit: Option<usize>,
) -> Result<WorkspaceSocket, String> {
    let (request, transport): (_, Pin<Box<dyn Transport>>) = match &reference.route {
        Route::Daemon { port, origin_port } => {
            let mut request = format!("ws://127.0.0.1:{port}/ws")
                .into_client_request()
                .map_err(|_| "hide_unavailable".to_owned())?;
            request.headers_mut().insert(
                ORIGIN,
                format!("http://127.0.0.1:{origin_port}")
                    .parse()
                    .map_err(|_| "hide_unavailable".to_owned())?,
            );
            let stream = tokio::net::TcpStream::connect(("127.0.0.1", *port))
                .await
                .map_err(|_| UNREACHABLE.to_owned())?;
            (request, Box::pin(stream))
        }
        Route::Node { socket } => {
            let socket = node_socket(socket).ok_or_else(|| "invalid_reference".to_owned())?;
            // The link's route answers only `/ws`, and only for a credential
            // the same node vouched for over the same link.
            let request = "ws://node/ws"
                .into_client_request()
                .map_err(|_| "hide_unavailable".to_owned())?;
            (request, Box::pin(node_stream(&socket)?))
        }
    };
    let config = response_limit.map(|limit| {
        tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(limit))
            .max_frame_size(Some(limit))
    });
    let (socket, _) = tokio_tungstenite::client_async_with_config(request, transport, config)
        .await
        .map_err(|_| UNREACHABLE.to_owned())?;
    Ok(socket)
}

/// The node socket a device reference names: an absolute path on this
/// machine, or nothing.
fn node_socket(wire: &str) -> Option<PathBuf> {
    hide_platform::path::from_wire(wire)
        .ok()
        .filter(|path| path.is_absolute())
}

/// Opens a stream through this device's node to the daemon behind it, as an
/// async stream. The node's local stream blocks, so two threads move its
/// bytes through an in-memory pipe; each ends when its side does, and the
/// first to end closes the node's stream, which ends the other.
fn node_stream(socket: &Path) -> Result<tokio::io::DuplexStream, String> {
    let stream = LocalStream::connect(socket).map_err(|_| UNREACHABLE.to_owned())?;
    let _ = stream.set_write_timeout(Some(TIMEOUT));
    stream
        .set_read_timeout(None)
        .map_err(|_| "hide_unavailable".to_owned())?;
    let mut writer = stream.duplicate();
    writeln!(writer, "{}", json!({"stream": true})).map_err(|_| UNREACHABLE.to_owned())?;
    let (ours, theirs) = tokio::io::duplex(hide_node_link::panes::MAX_CHUNK);
    let (mut outgoing, mut incoming) = tokio::io::split(theirs);
    let runtime = tokio::runtime::Handle::current();
    let reading = runtime.clone();
    let reader_shutdown = stream.shutdown_handle();
    let mut reader = stream;
    std::thread::Builder::new()
        .name("hide-node-stream-read".to_owned())
        .spawn(move || {
            let mut buffer = vec![0_u8; hide_node_link::panes::MAX_CHUNK];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        if reading
                            .block_on(incoming.write_all(&buffer[..read]))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            reader_shutdown.shutdown();
            let _ = reading.block_on(incoming.shutdown());
        })
        .map_err(|_| "hide_unavailable".to_owned())?;
    let writer_shutdown = writer.shutdown_handle();
    std::thread::Builder::new()
        .name("hide-node-stream-write".to_owned())
        .spawn(move || {
            let mut buffer = vec![0_u8; hide_node_link::panes::MAX_CHUNK];
            loop {
                match runtime.block_on(outgoing.read(&mut buffer)) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        if writer.write_all(&buffer[..read]).is_err() {
                            break;
                        }
                    }
                }
            }
            writer_shutdown.shutdown();
        })
        .map_err(|_| "hide_unavailable".to_owned())?;
    Ok(ours)
}

async fn exchange_response(
    reference: &Reference,
    payload: Value,
    request_id: &str,
) -> Result<(Value, WorkspaceSocket), String> {
    exchange_response_with_limit(reference, payload, request_id, None).await
}

async fn exchange_response_with_limit(
    reference: &Reference,
    payload: Value,
    request_id: &str,
    response_limit: Option<usize>,
) -> Result<(Value, WorkspaceSocket), String> {
    let mut socket = match response_limit {
        Some(limit) => connect_with_limit(reference, Some(limit)).await?,
        None => connect(reference).await?,
    };
    socket
        .send(Message::Text(
            json!({"token":reference.token,"schema_version":SCHEMA_VERSION})
                .to_string()
                .into(),
        ))
        .await
        .map_err(|_| "hide_unavailable".to_owned())?;
    socket
        .send(Message::Text(payload.to_string().into()))
        .await
        .map_err(|_| "hide_unavailable".to_owned())?;
    match socket.next().await {
        Some(Ok(Message::Text(text))) => {
            let value: Value =
                serde_json::from_str(&text).map_err(|_| "invalid_response".to_owned())?;
            if value["type"] != "workspace_result" || value["request_id"] != request_id {
                return Err("invalid_response".to_owned());
            }
            // The daemon checks the credential before it runs the command.
            if value["ok"] == false && value["reason"] == "credential_expired" {
                return Err("credential_expired".to_owned());
            }
            Ok((value, socket))
        }
        // A token the daemon does not hold: it expired, was revoked, or was
        // issued by a daemon that has since stopped.
        Some(Ok(Message::Close(Some(frame))))
            if u16::from(frame.code) == crate::server::CloseReason::InvalidToken.code() =>
        {
            Err("credential_expired".to_owned())
        }
        Some(Ok(Message::Close(_))) => Err("credential_rejected".to_owned()),
        _ => Err("hide_unavailable".to_owned()),
    }
}

async fn claim(socket: &mut WorkspaceSocket, path: &Path) -> Result<(), String> {
    socket
        .send(Message::Text(
            json!({"type":"workspace_claim"}).to_string().into(),
        ))
        .await
        .map_err(|_| "hide_unavailable".to_owned())?;
    match socket.next().await {
        Some(Ok(Message::Text(reply)))
            if serde_json::from_str::<Value>(&reply)
                .ok()
                .is_some_and(|answer| answer["type"] == "workspace_claimed") => {}
        _ => return Err("credential_expired".to_owned()),
    }
    let marker = path.with_extension("claimed");
    match private::create_new_file(&marker) {
        Ok(_) => Ok(()),
        Err(error)
            if error.kind() == std::io::ErrorKind::AlreadyExists
                && private::open_own_file(&marker, false).is_ok()
                && private::is_private(&marker).unwrap_or(false) =>
        {
            Ok(())
        }
        Err(_) => Err("reference_unavailable".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    // Mode bits widen the file for the refusal.
    #[cfg(unix)]
    #[test]
    fn reference_reader_refuses_symlink_and_open_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("reference.json");
        fs::write(&path, br#"{"token":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","port":12345,"origin_port":12345}"#).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&path, permissions.clone()).unwrap();
        assert!(matches!(
            read_reference(&path).unwrap().route,
            Route::Daemon {
                port: 12345,
                origin_port: 12345
            }
        ));
        let link = directory.path().join("link.json");
        symlink(&path, &link).unwrap();
        assert!(matches!(read_reference(&link), Err(reason) if reason == "credential_expired"));
        permissions.set_mode(0o644);
        fs::set_permissions(&path, permissions).unwrap();
        assert!(matches!(read_reference(&path), Err(reason) if reason == "invalid_reference"));
    }

    /// What a node writes up its link, one line at a time.
    struct Lines(Vec<u8>, mpsc::Sender<String>);

    impl Write for Lines {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(bytes);
            while let Some(end) = self.0.iter().position(|byte| *byte == b'\n') {
                let line = self.0.drain(..=end).collect::<Vec<_>>();
                let _ = self
                    .1
                    .send(String::from_utf8_lossy(&line).trim().to_owned());
            }
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A Unix socket path is limited to about a hundred bytes, and the node
    /// binds two folders below this one, so a long TMPDIR overflows it.
    fn short_dir() -> tempfile::TempDir {
        if cfg!(unix) {
            tempfile::Builder::new().prefix("hb").tempdir_in("/tmp")
        } else {
            tempfile::Builder::new().prefix("hb").tempdir()
        }
        .unwrap()
    }

    fn next_event(lines: &mpsc::Receiver<String>) -> Value {
        serde_json::from_str(&lines.recv_timeout(Duration::from_secs(10)).unwrap()).unwrap()
    }

    /// A device's node and a pane's `hide` meet over the system's local
    /// stream: the node sees the caller's pid and asks the pane's Herdr about
    /// it, and stopping the node removes it from this device.
    #[test]
    fn a_device_node_answers_this_device_until_it_stops() {
        let directory = short_dir();
        let bridges = directory.path().join("bridges");
        let herdr = directory.path().join("no-herdr.sock");
        let (sent, _lines) = mpsc::channel();
        let output = std::sync::Mutex::new(Lines(Vec::new(), sent));
        let panes = hide_host::panes::Panes::new();
        let request = json!({"pane_id": "w1:p1", "nonce": "0".repeat(32)});
        std::thread::scope(|scope| {
            let started = panes
                .start(scope, &output, &bridges, herdr.to_str().unwrap())
                .unwrap();
            // Both ends read and write the wire spelling, which on Windows is
            // the only one `from_wire` reads.
            assert!(
                node_socket(&started.socket).unwrap().starts_with(&bridges),
                "{}",
                started.socket
            );
            // No Herdr answers at the socket the node was given, so it cannot
            // read the pane; it got that far only by seeing who called.
            assert_eq!(
                bootstrap_bridges(&bridges, &request),
                Err("pane_unavailable".to_owned())
            );
            panes.stop();
        });
        panes.remove_folder();
        assert_eq!(bootstrap_bridges(&bridges, &request), Ok(None));
    }

    /// A command's bytes cross the node both ways, the first of them sent
    /// with the line that opens the stream, and the core closing the stream
    /// ends it for the command.
    #[test]
    fn a_command_stream_crosses_the_node_both_ways() {
        use base64::Engine as _;
        let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
        let directory = short_dir();
        let bridges = directory.path().join("bridges");
        let herdr = directory.path().join("no-herdr.sock");
        let (sent, lines) = mpsc::channel();
        let output = std::sync::Mutex::new(Lines(Vec::new(), sent));
        let panes = hide_host::panes::Panes::new();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        std::thread::scope(|scope| {
            let started = panes
                .start(scope, &output, &bridges, herdr.to_str().unwrap())
                .unwrap();
            let socket = node_socket(&started.socket).unwrap();
            runtime.block_on(async {
                let mut stream = node_stream(&socket).unwrap();
                stream.write_all(b"ping").await.unwrap();
                let opened = next_event(&lines);
                assert_eq!(opened["event"], "stream_open");
                let id = opened["stream"].as_u64().unwrap();
                let data = next_event(&lines);
                assert_eq!(
                    data,
                    json!({"event":"stream_data","stream":id,"data":encode(b"ping")})
                );
                panes.write_stream(id, &encode(b"pong")).unwrap();
                let mut answer = [0_u8; 4];
                stream.read_exact(&mut answer).await.unwrap();
                assert_eq!(&answer, b"pong");
                panes.close_stream(id).unwrap();
                let mut rest = Vec::new();
                stream.read_to_end(&mut rest).await.unwrap();
                assert!(rest.is_empty());
                assert_eq!(
                    next_event(&lines),
                    json!({"event":"stream_closed","stream":id})
                );
            });
            panes.stop();
        });
        panes.remove_folder();
    }

    #[test]
    fn action_result_survives_a_lost_claim_ack() {
        let (sender, receiver) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                sender.send(listener.local_addr().unwrap().port()).unwrap();
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                let _ = socket.next().await;
                let _ = socket.next().await;
                socket
                    .send(Message::Text(
                        json!({
                            "type":"workspace_result","request_id":"1-retry","ok":true,
                            "result":{"view_id":"view-1"}
                        })
                        .to_string()
                        .into(),
                    ))
                    .await
                    .unwrap();
                socket.close(None).await.unwrap();
            });
        });
        let port = receiver.recv().unwrap();
        let reference = Reference {
            token: "a".repeat(64),
            route: Route::Daemon {
                port,
                origin_port: port,
            },
        };
        let directory = tempfile::tempdir().unwrap();
        let answer = run_exchange(
            &directory.path().join("reference.json"),
            &reference,
            json!({"type":"workspace_action","request_id":"1-retry"}),
            "1-retry",
            true,
        )
        .unwrap();
        assert_eq!(answer["result"]["view_id"], "view-1");
        server.join().unwrap();
    }
}
