//! Pane bootstrap on a consented SSH device. The helper listens only on an
//! owner-only Unix socket and lives only while its SSH exec stdin is open.
//! The invoking pane is attested from the socket's kernel peer PID; the local
//! daemon owns the capability and answers over this exec channel.

use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use hide_herdr_client::{UnixSocketConnector, request_with_connector};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::pane_peer;

const HERDR_TIMEOUT: Duration = Duration::from_secs(2);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CONTROL_LINE: u64 = 4096;

#[derive(Deserialize)]
pub struct Init {
    pub bridge_dir: Option<PathBuf>,
    pub herdr_socket: PathBuf,
    pub port: u16,
    pub origin_port: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PaneIdentity {
    pub terminal_id: String,
    pub shell_pid: i32,
    pub shell_started: u64,
}

#[derive(Deserialize)]
struct Bootstrap {
    pane_id: String,
    nonce: String,
    #[serde(default)]
    one_shot: bool,
}

pub fn inspect(socket: &Path, pane_id: &str) -> Result<PaneIdentity, &'static str> {
    if !socket.is_absolute() || pane_id.is_empty() || pane_id.len() > 256 {
        return Err("invalid_request");
    }
    let connector = UnixSocketConnector::new(socket);
    let process = request_with_connector(
        &connector,
        "pane.process_info",
        json!({"pane_id":pane_id}),
        HERDR_TIMEOUT,
    )
    .map_err(|_| "pane_unavailable")?;
    let shell = process
        .pointer("/process_info/shell_pid")
        .and_then(Value::as_u64)
        .filter(|pid| *pid > 0 && *pid <= i32::MAX as u64)
        .ok_or("pane_unavailable")? as i32;
    let started = pane_peer::process_start(shell).ok_or("pane_unavailable")?;
    let pane = request_with_connector(
        &connector,
        "pane.get",
        json!({"pane_id":pane_id}),
        HERDR_TIMEOUT,
    )
    .map_err(|_| "pane_unavailable")?;
    let terminal_id = pane
        .pointer("/pane/terminal_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("pane_unavailable")?
        .to_owned();
    Ok(PaneIdentity {
        terminal_id,
        shell_pid: shell,
        shell_started: started,
    })
}

fn validate_dir(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bridge directory is not absolute",
        ));
    }
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (!metadata.is_dir() || metadata.file_type().is_symlink())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bridge directory is unsafe",
        ));
    }
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "bridge directory is not owned by this account",
        ));
    }
    Ok(())
}

fn write_line(output: &Mutex<impl Write>, value: Value) -> io::Result<()> {
    let mut output = output
        .lock()
        .map_err(|_| io::Error::other("bridge output lock poisoned"))?;
    writeln!(output, "{value}")?;
    output.flush()
}

fn answer_client(stream: &mut UnixStream, result: Result<PathBuf, String>) {
    let answer = match result {
        Ok(reference) => json!({"ok":true,"reference":reference}),
        Err(reason) => json!({"ok":false,"reason":reason}),
    };
    let _ = writeln!(stream, "{answer}");
}

fn serve_client(
    mut stream: UnixStream,
    id: u64,
    init: &Init,
    dir: &Path,
    output: &Mutex<impl Write>,
    replies: &Mutex<mpsc::Receiver<Value>>,
) {
    let result: Result<PathBuf, String> = (|| {
        let peer = pane_peer::peer_pid(&stream).ok_or("caller_unavailable")?;
        stream
            .set_read_timeout(Some(CLIENT_TIMEOUT))
            .map_err(|_| "bridge_unavailable")?;
        stream
            .set_write_timeout(Some(CLIENT_TIMEOUT))
            .map_err(|_| "bridge_unavailable")?;
        let mut line = String::new();
        BufReader::new(&stream)
            .take(MAX_CONTROL_LINE)
            .read_line(&mut line)
            .map_err(|_| "invalid_request")?;
        let request: Bootstrap = serde_json::from_str(&line).map_err(|_| "invalid_request")?;
        if request.nonce.len() != 32 || !request.nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("invalid_nonce".to_owned());
        }
        let identity = inspect(&init.herdr_socket, &request.pane_id)?;
        if !pane_peer::descends_from(peer, identity.shell_pid) {
            return Err("caller_not_in_pane".to_owned());
        }
        write_line(
            output,
            json!({
                "type":"attest", "id":id, "pane_id":request.pane_id,
                "terminal_id":identity.terminal_id, "shell_pid":identity.shell_pid,
                "shell_started":identity.shell_started, "nonce":request.nonce,
                "one_shot":request.one_shot,
            }),
        )
        .map_err(|_| "bridge_unavailable")?;
        let answer = replies
            .lock()
            .map_err(|_| "bridge_unavailable")?
            .recv_timeout(CLIENT_TIMEOUT)
            .map_err(|_| "bridge_unavailable")?;
        if answer["id"].as_u64() != Some(id) {
            return Err("invalid_response".to_owned());
        }
        if answer["ok"] != true {
            return Err(answer["reason"]
                .as_str()
                .unwrap_or("bootstrap_refused")
                .to_owned());
        }
        let token = answer["token"]
            .as_str()
            .filter(|token| token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("invalid_response")?;
        let path = dir.join(format!("{}.json", request.nonce));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|_| "reference_unavailable")?;
        if write!(
            file,
            "{}",
            json!({"token":token,"port":init.port,"origin_port":init.origin_port})
        )
        .is_err()
            || file.sync_all().is_err()
        {
            let _ = fs::remove_file(&path);
            return Err("reference_unavailable".to_owned());
        }
        Ok(path)
    })();
    answer_client(&mut stream, result);
}

pub fn serve(mut input: impl BufRead + Send, output: impl Write + Send) -> io::Result<()> {
    let mut line = String::new();
    input.by_ref().take(MAX_CONTROL_LINE).read_line(&mut line)?;
    let init: Init = serde_json::from_str(&line).map_err(io::Error::other)?;
    if init.port == 0 || init.origin_port == 0 || !init.herdr_socket.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bridge init is invalid",
        ));
    }
    let bridge_dir = match &init.bridge_dir {
        Some(path) => path.clone(),
        None => {
            let home =
                std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is unavailable"))?;
            PathBuf::from(home).join(".local/state/hide/workspace-bridges")
        }
    };
    validate_dir(&bridge_dir)?;
    let dir = tempfile::Builder::new()
        .prefix("bridge-")
        .tempdir_in(&bridge_dir)?;
    let socket = dir.path().join("bootstrap.sock");
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let output = Mutex::new(output);
    write_line(&output, json!({"type":"ready","socket":socket}))?;
    let (sender, receiver) = mpsc::sync_channel::<Value>(1);
    let replies = Mutex::new(receiver);
    let closed = AtomicBool::new(false);
    let next_id = AtomicU64::new(1);
    let busy = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let mut line = String::new();
            loop {
                line.clear();
                match input.by_ref().take(MAX_CONTROL_LINE).read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => match serde_json::from_str::<Value>(&line) {
                        Ok(value) => {
                            if sender.send(value).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    },
                }
            }
            closed.store(true, Ordering::Release);
        });
        while !closed.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    if busy.swap(true, Ordering::AcqRel) {
                        answer_client(&mut stream, Err("bridge_busy".to_owned()));
                        continue;
                    }
                    let id = next_id.fetch_add(1, Ordering::Relaxed);
                    let output = &output;
                    let replies = &replies;
                    let busy = &busy;
                    let init = &init;
                    let dir_path = dir.path();
                    scope.spawn(move || {
                        serve_client(stream, id, init, dir_path, output, replies);
                        busy.store(false, Ordering::Release);
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    })
}
