//! Pane-scoped Workspace CLI transport. Bootstrap attests the caller over a
//! local Unix socket; commands and results use hided's authenticated `/ws`.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use herdr_core::workspace_control::Action;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::ORIGIN;

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
    let pane_id = env
        .pane_id
        .as_deref()
        .ok_or_else(|| "pane_not_connected".to_owned())?;
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
    let socket = env.state_dir.join("pane-bootstrap.sock");
    let mut stream = UnixStream::connect(socket).map_err(|_| "hide_unavailable".to_owned())?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|_| "hide_unavailable".to_owned())?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|_| "hide_unavailable".to_owned())?;
    writeln!(stream, "{request}").map_err(|_| "hide_unavailable".to_owned())?;
    read_bootstrap_answer(&mut stream)
}

fn read_bootstrap_answer(stream: &mut UnixStream) -> Result<PathBuf, String> {
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
        .map(PathBuf::from)
        .ok_or_else(|| "reference_unavailable".to_owned())
}

fn bootstrap_remote(env: &Env, request: &Value) -> Result<Option<PathBuf>, String> {
    let bridge_dir = env
        .workspace_bridge_dir
        .clone()
        .unwrap_or_else(|| env.home.join(".local/state/hide/workspace-bridges"));
    let metadata = match fs::symlink_metadata(&bridge_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("bridge_unavailable".to_owned()),
    };
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
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
        let socket = entry.path().join("bootstrap.sock");
        let Ok(metadata) = fs::symlink_metadata(&socket) else {
            continue;
        };
        if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
            continue;
        }
        let Ok(mut stream) = UnixStream::connect(&socket) else {
            continue;
        };
        seen += 1;
        if seen > 16 {
            return Err("bridge_limit".to_owned());
        }
        let _ = stream.set_read_timeout(Some(REMOTE_BOOTSTRAP_TIMEOUT));
        let _ = stream.set_write_timeout(Some(REMOTE_BOOTSTRAP_TIMEOUT));
        if writeln!(stream, "{request}").is_err() {
            reason = Some("bridge_unavailable".to_owned());
            continue;
        }
        match read_bootstrap_answer(&mut stream) {
            Ok(path) if path.starts_with(&bridge_dir) => {
                if success.is_some() {
                    let _ = fs::remove_file(&path);
                    if let Some(first) = success {
                        let _ = fs::remove_file(first);
                    }
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
    // Verify the opened inode, so replacing the path with a symlink between
    // metadata and read cannot make this helper read an unrelated file.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "credential_expired".to_owned())?;
    let metadata = file
        .metadata()
        .map_err(|_| "credential_expired".to_owned())?;
    if !metadata.file_type().is_file()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.len() > 4096
    {
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
    let reference = read_reference(path)?;
    let request_id = fresh_request_id()?;
    run_exchange(
        path,
        &reference,
        json!({"type":"workspace_query","request_id":request_id,"query":query}),
        &request_id,
    )
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
) -> Result<Value, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "request_unavailable".to_owned())?;
    runtime.block_on(async {
        tokio::time::timeout(TIMEOUT, exchange(path, reference, payload, request_id))
            .await
            .map_err(|_| "request_timeout".to_owned())?
    })
}

async fn exchange(
    path: &Path,
    reference: &Reference,
    payload: Value,
    request_id: &str,
) -> Result<Value, String> {
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
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&marker)
            {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err("reference_unavailable".to_owned()),
            }
            Ok(value)
        }
        Some(Ok(Message::Close(_))) => Err("credential_rejected".to_owned()),
        _ => Err("hide_unavailable".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn reference_reader_refuses_symlink_and_open_permissions() {
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
}
