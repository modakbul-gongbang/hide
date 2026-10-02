//! Pane bootstrap on a consented SSH device. The helper listens only on an
//! owner-only Unix socket and lives only while its SSH exec stdin is open.
//! The invoking pane is attested from the socket's kernel peer PID; the local
//! daemon owns the capability and answers over this exec channel.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use hide_herdr_client::{LocalSocketConnector, request_with_connector};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::pane_peer;

const HERDR_TIMEOUT: Duration = Duration::from_secs(2);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_CONTROL_LINE: u64 = 4096;
const UNCLAIMED_LIFETIME: Duration = Duration::from_secs(30);
const REFERENCE_LIFETIME: Duration = Duration::from_secs(8 * 60 * 60);

struct IssuedReference {
    token: String,
    created: Instant,
    holder: Option<(i32, u64)>,
    shell_pid: i32,
    shell_started: u64,
}

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
    let connector = LocalSocketConnector::new(socket);
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

fn answer_client(stream: &mut UnixStream, result: Result<PathBuf, String>) -> io::Result<()> {
    let answer = match result {
        Ok(reference) => json!({"ok":true,"reference":reference}),
        Err(reason) => json!({"ok":false,"reason":reason}),
    };
    writeln!(stream, "{answer}")
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn receive_reply(replies: &mpsc::Receiver<Value>, id: u64) -> Result<Value, &'static str> {
    let deadline = Instant::now() + CLIENT_TIMEOUT;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("bridge_unavailable")?;
        let answer = replies
            .recv_timeout(remaining)
            .map_err(|_| "bridge_unavailable")?;
        match answer["id"].as_u64() {
            Some(answer_id) if answer_id < id => continue,
            Some(answer_id) if answer_id == id => return Ok(answer),
            _ => return Err("invalid_response"),
        }
    }
}

fn sweep_references(
    issued: &Mutex<HashMap<PathBuf, IssuedReference>>,
    output: &Mutex<impl Write>,
) -> io::Result<()> {
    let mut revoked = Vec::new();
    {
        let mut issued = issued
            .lock()
            .map_err(|_| io::Error::other("bridge reference lock poisoned"))?;
        let stale = issued
            .iter()
            .filter_map(|(path, reference)| {
                let present = path.is_file();
                let expired = reference.created.elapsed() >= REFERENCE_LIFETIME
                    || (!path.with_extension("claimed").is_file()
                        && reference.created.elapsed() >= UNCLAIMED_LIFETIME);
                let holder_gone = reference
                    .holder
                    .is_some_and(|(pid, started)| pane_peer::process_start(pid) != Some(started));
                let pane_gone =
                    pane_peer::process_start(reference.shell_pid) != Some(reference.shell_started);
                (!present || expired || holder_gone || pane_gone).then_some(path.clone())
            })
            .collect::<Vec<_>>();
        for path in stale {
            remove_if_present(&path)?;
            remove_if_present(&path.with_extension("claimed"))?;
            if let Some(reference) = issued.remove(&path) {
                revoked.push(reference.token);
            }
        }
    }
    for token in revoked {
        write_line(output, json!({"type":"revoke","token":token}))?;
    }
    Ok(())
}

fn serve_client(
    mut stream: UnixStream,
    id: u64,
    init: &Init,
    dir: &Path,
    output: &Mutex<impl Write>,
    replies: &Mutex<mpsc::Receiver<Value>>,
    issued: &Mutex<HashMap<PathBuf, IssuedReference>>,
) {
    let mut issued_token = None;
    let mut token_new = false;
    let mut reference_new = false;
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
        let holder = if request.one_shot {
            Some((
                peer,
                pane_peer::process_start(peer).ok_or("caller_unavailable")?,
            ))
        } else {
            None
        };
        sweep_references(issued, output).map_err(|_| "bridge_unavailable")?;
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
        let answer = receive_reply(&*replies.lock().map_err(|_| "bridge_unavailable")?, id)?;
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
        issued_token = Some(token.to_owned());
        token_new = answer["issued_new"] == true;
        let mut issued = issued.lock().map_err(|_| "bridge_unavailable")?;
        if !request.one_shot
            && let Some((path, _)) = issued.iter().find(|(path, reference)| {
                reference.token == token && path.is_file() && reference.holder.is_none()
            })
        {
            return Ok(path.clone());
        }
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
        issued.insert(
            path.clone(),
            IssuedReference {
                token: token.to_owned(),
                created: Instant::now(),
                holder,
                shell_pid: identity.shell_pid,
                shell_started: identity.shell_started,
            },
        );
        reference_new = true;
        Ok(path)
    })();
    let delivered = answer_client(&mut stream, result.clone()).is_ok();
    if !delivered || result.is_err() {
        if reference_new && let Ok(path) = result {
            let _ = fs::remove_file(&path);
            if let Ok(mut issued) = issued.lock() {
                issued.remove(&path);
            }
        }
        if token_new && let Some(token) = issued_token {
            let _ = write_line(output, json!({"type":"revoke","id":id,"token":token}));
        }
    }
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
    let issued = Mutex::new(HashMap::new());
    let closed = AtomicBool::new(false);
    let next_id = AtomicU64::new(1);
    let busy = AtomicBool::new(false);
    let mut last_sweep = Instant::now();
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
                        let _ = answer_client(&mut stream, Err("bridge_busy".to_owned()));
                        continue;
                    }
                    let id = next_id.fetch_add(1, Ordering::Relaxed);
                    let output = &output;
                    let replies = &replies;
                    let issued = &issued;
                    let busy = &busy;
                    let init = &init;
                    let dir_path = dir.path();
                    scope.spawn(move || {
                        serve_client(stream, id, init, dir_path, output, replies, issued);
                        busy.store(false, Ordering::Release);
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if last_sweep.elapsed() >= Duration::from_secs(5) {
                        sweep_references(&issued, &output)?;
                        last_sweep = Instant::now();
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_unclaimed_reference_is_revoked_but_a_claimed_session_remains() {
        let dir = tempfile::tempdir().unwrap();
        let shell_pid = std::process::id() as i32;
        let shell_started = pane_peer::process_start(shell_pid).unwrap();
        let abandoned = dir.path().join(format!("{}.json", "a".repeat(32)));
        let claimed = dir.path().join(format!("{}.json", "b".repeat(32)));
        fs::write(&abandoned, "abandoned").unwrap();
        fs::write(&claimed, "claimed").unwrap();
        fs::write(claimed.with_extension("claimed"), "").unwrap();
        let issued = Mutex::new(HashMap::from([
            (
                abandoned.clone(),
                IssuedReference {
                    token: "a".to_owned(),
                    created: Instant::now() - UNCLAIMED_LIFETIME - Duration::from_secs(1),
                    holder: None,
                    shell_pid,
                    shell_started,
                },
            ),
            (
                claimed.clone(),
                IssuedReference {
                    token: "b".to_owned(),
                    created: Instant::now() - UNCLAIMED_LIFETIME - Duration::from_secs(1),
                    holder: None,
                    shell_pid,
                    shell_started,
                },
            ),
        ]));
        let output = Mutex::new(Vec::new());
        sweep_references(&issued, &output).unwrap();
        assert!(!abandoned.exists());
        assert!(claimed.exists());
        assert!(
            String::from_utf8(output.into_inner().unwrap())
                .unwrap()
                .contains("\"token\":\"a\"")
        );
    }

    #[test]
    fn deleted_reference_and_late_reply_do_not_keep_a_grant_or_poison_the_next_request() {
        let dir = tempfile::tempdir().unwrap();
        let shell_pid = std::process::id() as i32;
        let shell_started = pane_peer::process_start(shell_pid).unwrap();
        let path = dir.path().join(format!("{}.json", "c".repeat(32)));
        let issued = Mutex::new(HashMap::from([(
            path.clone(),
            IssuedReference {
                token: "c".to_owned(),
                created: Instant::now(),
                holder: None,
                shell_pid,
                shell_started,
            },
        )]));
        let output = Mutex::new(Vec::new());
        sweep_references(&issued, &output).unwrap();
        assert!(
            String::from_utf8(output.into_inner().unwrap())
                .unwrap()
                .contains("\"token\":\"c\"")
        );

        let (sender, receiver) = mpsc::channel();
        sender.send(json!({"id":1,"ok":true})).unwrap();
        sender.send(json!({"id":2,"ok":true})).unwrap();
        assert_eq!(receive_reply(&receiver, 2).unwrap()["id"], 2);
    }

    #[test]
    fn a_killed_one_shot_caller_loses_its_claimed_reference() {
        let dir = tempfile::tempdir().unwrap();
        let shell_pid = std::process::id() as i32;
        let shell_started = pane_peer::process_start(shell_pid).unwrap();
        let path = dir.path().join(format!("{}.json", "d".repeat(32)));
        fs::write(&path, "reference").unwrap();
        fs::write(path.with_extension("claimed"), "").unwrap();
        let issued = Mutex::new(HashMap::from([(
            path.clone(),
            IssuedReference {
                token: "d".to_owned(),
                created: Instant::now(),
                holder: Some((i32::MAX, 1)),
                shell_pid,
                shell_started,
            },
        )]));
        let output = Mutex::new(Vec::new());
        sweep_references(&issued, &output).unwrap();
        assert!(!path.exists());
        assert!(!path.with_extension("claimed").exists());
        assert!(
            String::from_utf8(output.into_inner().unwrap())
                .unwrap()
                .contains("\"token\":\"d\"")
        );
    }
}
