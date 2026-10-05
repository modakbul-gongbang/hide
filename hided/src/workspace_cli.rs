//! Pane-scoped Workspace CLI transport. Bootstrap attests the caller over a
//! local socket; commands and results use hided's authenticated `/ws`.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use herdr_core::workspace_control::Action;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::ORIGIN;

use hide_platform::fs::private;
use hide_platform::ipc::LocalStream;

use crate::env::Env;
use crate::pane_auth::Reference;
use crate::state_file::SCHEMA_VERSION;

const TIMEOUT: Duration = Duration::from_secs(10);
const REMOTE_BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(25);

/// An auto-bootstrapped direct CLI owns its reference even if transport fails.
pub struct OneShotReference(pub PathBuf);

impl Drop for OneShotReference {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        let _ = fs::remove_file(self.0.with_extension("claimed"));
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

/// Asks each workspace bridge a remote daemon runs on this device
/// (`hide_host::workspace_bridge`) for this pane's reference.
fn bootstrap_remote(env: &Env, request: &Value) -> Result<Option<PathBuf>, String> {
    let bridge_dir = env
        .workspace_bridge_dir
        .clone()
        .unwrap_or_else(|| hide_kit::layout::workspace_bridges(&env.state_dir));
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
    if reference.port == 0 || reference.origin_port == 0 {
        return Err("invalid_reference".to_owned());
    }
    Ok(reference)
}

pub fn request(path: &Path, query: &str) -> Result<Value, String> {
    request_query(path, query, None)
}

pub fn browser_connect(path: &Path, display_id: Option<&str>) -> Result<Value, String> {
    request_query(path, "browser_connect", display_id)
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

/// Losing the final capability claim cannot turn durable success into retry.
pub fn request_delivery(
    path: &Path,
    command: herdr_core::delivery::Command,
    hint: Option<&str>,
) -> Result<Value, String> {
    let reference = read_reference(path)?;
    let request_id = fresh_request_id()?;
    run_exchange(
        path,
        &reference,
        json!({
            "type":"delivery", "request_id":request_id, "command":command, "caller_pane":hint
        }),
        &request_id,
        true,
    )
}

/// The relay socket for one `hide browser` page command: the same scoped
/// handshake and claim as every Workspace request, after which the socket
/// carries CDP frames to the caller's display. A refusal keeps the daemon's
/// reason and next action.
/// The relay socket, and whether the display is its area's selected View.
pub(crate) async fn browser_relay(
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

pub fn request_action(path: &Path, action: Action, request_id: &str) -> Result<Value, String> {
    let reference = read_reference(path)?;
    if !valid_request_id(request_id) {
        return Err("invalid_request_id".to_owned());
    }
    run_exchange(
        path,
        &reference,
        json!({"type":"workspace_action","request_id":request_id,"command":action}),
        request_id,
        true,
    )
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
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "request_unavailable".to_owned())?;
    let (value, mut socket) = runtime.block_on(async {
        tokio::time::timeout(TIMEOUT, exchange_response(reference, payload, request_id))
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

pub(crate) type WorkspaceSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn exchange_response(
    reference: &Reference,
    payload: Value,
    request_id: &str,
) -> Result<(Value, WorkspaceSocket), String> {
    let mut request = format!("ws://127.0.0.1:{}/ws", reference.port)
        .into_client_request()
        .map_err(|_| "hide_unavailable".to_owned())?;
    request.headers_mut().insert(
        ORIGIN,
        format!("http://127.0.0.1:{}", reference.origin_port)
            .parse()
            .map_err(|_| "hide_unavailable".to_owned())?,
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|_| "hide_unavailable".to_owned())?;
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
            Ok((value, socket))
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
        assert_eq!(read_reference(&path).unwrap().port, 12345);
        let link = directory.path().join("link.json");
        symlink(&path, &link).unwrap();
        assert!(matches!(read_reference(&link), Err(reason) if reason == "credential_expired"));
        permissions.set_mode(0o644);
        fs::set_permissions(&path, permissions).unwrap();
        assert!(matches!(read_reference(&path), Err(reason) if reason == "invalid_reference"));
    }

    /// What the bridge writes to its exec channel, one line at a time.
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

    /// A device's bridge and a pane's `hide` meet over the system's local
    /// stream: the bridge sees the caller's pid and asks the pane's Herdr
    /// about it, and the end of its exec channel ends it.
    #[test]
    fn a_remote_bridge_answers_this_device_and_ends_with_its_channel() {
        // A Unix socket path is limited to about a hundred bytes, and the
        // bridge binds two folders below this one, so a long TMPDIR overflows it.
        let directory = if cfg!(unix) {
            tempfile::Builder::new().prefix("hb").tempdir_in("/tmp")
        } else {
            tempfile::Builder::new().prefix("hb").tempdir()
        }
        .unwrap();
        let bridges = directory.path().join("bridges");
        let (channel, mut daemon) = std::io::pipe().unwrap();
        let (sent, lines) = mpsc::channel();
        let init = json!({
            "bridge_dir": hide_platform::path::to_wire(&bridges).unwrap(),
            "herdr_socket": directory.path().join("no-herdr.sock"),
            "port": 1, "origin_port": 2,
        });
        writeln!(daemon, "{init}").unwrap();
        let (ended, bridge) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = ended.send(hide_host::workspace_bridge::serve(
                BufReader::new(channel),
                Lines(Vec::new(), sent),
            ));
        });
        let ready = match lines.recv_timeout(Duration::from_secs(10)) {
            Ok(line) => line,
            // The bridge drops its output before it hands back its result,
            // so wait for the result rather than read what is there now.
            Err(_) => panic!(
                "the bridge never became ready: {:?}",
                bridge.recv_timeout(Duration::from_secs(1))
            ),
        };
        let ready: Value = serde_json::from_str(&ready).unwrap();
        assert_eq!(ready["type"], "ready");
        // Both ends of the line read and write the wire spelling, which on
        // Windows is the only one `from_wire` reads.
        let socket = ready["socket"].as_str().unwrap();
        assert!(
            hide_platform::path::from_wire(socket)
                .unwrap()
                .starts_with(&bridges),
            "{socket}"
        );
        let home = directory.path().to_str().unwrap().to_owned();
        let bridge_dir = bridges.to_str().unwrap().to_owned();
        let env = crate::env::load_from(|key| match key {
            crate::env::HOME => Some(home.clone()),
            crate::env::HIDE_WORKSPACE_BRIDGE_DIR => Some(bridge_dir.clone()),
            _ => None,
        })
        .unwrap();
        // No Herdr answers at the socket the bridge was given, so it cannot
        // read the pane; it got that far only by seeing who called.
        let request = json!({"pane_id": "w1:p1", "nonce": "0".repeat(32)});
        assert_eq!(
            bootstrap_remote(&env, &request),
            Err("pane_unavailable".to_owned())
        );
        drop(daemon);
        bridge
            .recv_timeout(Duration::from_secs(10))
            .expect("the bridge ends with its channel")
            .unwrap();
        assert_eq!(bootstrap_remote(&env, &request), Ok(None));
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
            port,
            origin_port: port,
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
