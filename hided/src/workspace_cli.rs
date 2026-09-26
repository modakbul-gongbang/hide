//! Pane-scoped Workspace CLI transport. Bootstrap attests the caller over a
//! local Unix socket; commands and results use hided's authenticated `/ws`.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::ORIGIN;

use crate::env::Env;
use crate::pane_auth::Reference;
use crate::state_file::SCHEMA_VERSION;

const TIMEOUT: Duration = Duration::from_secs(10);

/// An auto-bootstrapped direct CLI owns its reference even if transport fails.
pub struct OneShotReference(pub PathBuf);

impl Drop for OneShotReference {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn bootstrap(env: &Env, one_shot: bool) -> Result<PathBuf, String> {
    let pane_id = env
        .pane_id
        .as_deref()
        .ok_or_else(|| "pane_not_connected".to_owned())?;
    let socket = env.state_dir.join("pane-bootstrap.sock");
    let mut stream = UnixStream::connect(socket).map_err(|_| "hide_unavailable".to_owned())?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|_| "hide_unavailable".to_owned())?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|_| "hide_unavailable".to_owned())?;
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce).map_err(|_| "reference_unavailable".to_owned())?;
    writeln!(
        stream,
        "{}",
        json!({"pane_id":pane_id,"nonce":hex::encode(nonce),"one_shot":one_shot})
    )
    .map_err(|_| "hide_unavailable".to_owned())?;
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
    Ok(reference)
}

pub fn request(path: &Path, query: &str) -> Result<Value, String> {
    let reference = read_reference(path)?;
    let mut id = [0u8; 16];
    getrandom::getrandom(&mut id).map_err(|_| "request_unavailable".to_owned())?;
    let request_id = hex::encode(id);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "request_unavailable".to_owned())?;
    runtime.block_on(async {
        tokio::time::timeout(TIMEOUT, exchange(&reference, query, &request_id))
            .await
            .map_err(|_| "request_timeout".to_owned())?
    })
}

async fn exchange(reference: &Reference, query: &str, request_id: &str) -> Result<Value, String> {
    let mut request = format!("ws://127.0.0.1:{}/ws", reference.port)
        .into_client_request()
        .map_err(|_| "hide_unavailable".to_owned())?;
    request.headers_mut().insert(
        ORIGIN,
        format!("http://127.0.0.1:{}", reference.port)
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
        .send(Message::Text(
            json!({"type":"workspace_query","request_id":request_id,"query":query})
                .to_string()
                .into(),
        ))
        .await
        .map_err(|_| "hide_unavailable".to_owned())?;
    match socket.next().await {
        Some(Ok(Message::Text(text))) => {
            let value: Value =
                serde_json::from_str(&text).map_err(|_| "invalid_response".to_owned())?;
            if value["type"] != "workspace_result" || value["request_id"] != request_id {
                return Err("invalid_response".to_owned());
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
        fs::write(&path, br#"{"token":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","port":12345}"#).unwrap();
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
