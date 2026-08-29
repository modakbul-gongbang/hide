//! Live herdr integration: session snapshot polling over the local API socket
//! and pane byte transport through `herdr pane attach` under a PTY.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde_json::{Value, json};

use crate::ffi::ChangeNotifier;
use crate::runtime::Runtime;
use crate::sidebar::{SessionAgentPayload, SessionSnapshotPayload};

/// Herdr API protocol revision this core speaks. A mismatch is a hard,
/// explicit failure instead of a partially working sidebar.
pub const HERDR_PROTOCOL_REVISION: u64 = 21;

const API_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Everything an attach spawn needs from the live configuration.
#[derive(Clone)]
pub struct LiveContext {
    pub socket_path: PathBuf,
    pub herdr_bin: Option<PathBuf>,
    pub runtime: Weak<Mutex<Runtime>>,
    pub notifier: ChangeNotifier,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionFetchError {
    /// The socket file itself does not exist: the herdr server is not running.
    SocketMissing(String),
    /// The socket exists but the request failed (connect, timeout, IO).
    Unreachable(String),
    /// The server answered with an incompatible protocol revision.
    Protocol(String),
    /// The server answered but the payload did not match the expected shape.
    Malformed(String),
}

impl SessionFetchError {
    pub fn state(&self) -> &'static str {
        match self {
            Self::SocketMissing(_) => "socket_missing",
            Self::Unreachable(_) => "unreachable",
            Self::Protocol(_) => "protocol_mismatch",
            Self::Malformed(_) => "malformed",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::SocketMissing(message)
            | Self::Unreachable(message)
            | Self::Protocol(message)
            | Self::Malformed(message) => message,
        }
    }
}

/// Installs the live context on the runtime and starts the session poller.
pub fn install(
    runtime: &Arc<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    socket_path: &str,
    herdr_bin: Option<&str>,
) {
    let context = LiveContext {
        socket_path: PathBuf::from(socket_path),
        herdr_bin: herdr_bin.map(PathBuf::from),
        runtime: Arc::downgrade(runtime),
        notifier: notifier.clone(),
    };
    if let Ok(mut guard) = runtime.lock() {
        guard.set_live(context.clone());
    }
    spawn_session_poller(context);
}

fn spawn_session_poller(context: LiveContext) {
    let result = thread::Builder::new()
        .name("herdr-core-session-poller".to_owned())
        .spawn(move || {
            loop {
                let fetched = fetch_session(&context.socket_path);
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_session(fetched),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
                thread::sleep(POLL_INTERVAL);
            }
        });
    if let Err(error) = result {
        eprintln!(
            "{}",
            json!({
                "component": "live",
                "kind": "poller.spawn_failed",
                "message": error.to_string(),
            })
        );
    }
}

pub fn fetch_session(socket_path: &Path) -> Result<SessionSnapshotPayload, SessionFetchError> {
    if !socket_path.exists() {
        return Err(SessionFetchError::SocketMissing(format!(
            "Herdr socket file does not exist at {}; the herdr server is not running",
            socket_path.display()
        )));
    }
    let result = request(socket_path, "session.snapshot", json!({}))
        .map_err(SessionFetchError::Unreachable)?;
    let snapshot = result
        .get("snapshot")
        .ok_or_else(|| SessionFetchError::Malformed("response is missing snapshot".to_owned()))?;
    project_session(snapshot)
}

/// Maps the herdr wire snapshot into the sidebar session payload. Tokens are
/// passed through verbatim so the unseen-vs-acknowledged state rules
/// (INV-herdr-unseen-token) stay owned by the sidebar projection.
pub fn project_session(snapshot: &Value) -> Result<SessionSnapshotPayload, SessionFetchError> {
    let protocol = snapshot
        .get("protocol")
        .and_then(Value::as_u64)
        .ok_or_else(|| SessionFetchError::Malformed("snapshot is missing protocol".to_owned()))?;
    if protocol != HERDR_PROTOCOL_REVISION {
        return Err(SessionFetchError::Protocol(format!(
            "Herdr protocol revision {protocol} does not match required {HERDR_PROTOCOL_REVISION}"
        )));
    }

    let workspace_labels: std::collections::BTreeMap<&str, &str> = snapshot
        .get("workspaces")
        .and_then(Value::as_array)
        .map(|workspaces| {
            workspaces
                .iter()
                .filter_map(|workspace| {
                    Some((
                        workspace.get("workspace_id")?.as_str()?,
                        workspace.get("label")?.as_str()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    let agents = snapshot
        .get("agents")
        .and_then(Value::as_array)
        .ok_or_else(|| SessionFetchError::Malformed("snapshot is missing agents".to_owned()))?
        .iter()
        .filter_map(|agent| {
            let pane_id = agent.get("pane_id")?.as_str()?.to_owned();
            let workspace_id = agent.get("workspace_id").and_then(Value::as_str);
            Some(SessionAgentPayload {
                id: Some(pane_id.clone()),
                pane_id: Some(pane_id),
                workspace_label: workspace_id
                    .and_then(|id| workspace_labels.get(id).copied())
                    .or(workspace_id)
                    .map(str::to_owned),
                cwd: agent.get("cwd").and_then(Value::as_str).map(str::to_owned),
                agent: agent
                    .get("agent")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                agent_status: agent
                    .get("agent_status")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                tokens: agent
                    .get("tokens")
                    .and_then(Value::as_object)
                    .map(|tokens| tokens.clone().into_iter().collect())
                    .unwrap_or_default(),
            })
        })
        .collect();

    Ok(SessionSnapshotPayload { agents })
}

fn request(socket_path: &Path, method: &str, params: Value) -> Result<Value, String> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|error| format!("connect failed for {}: {error}", socket_path.display()))?;
    stream
        .set_read_timeout(Some(API_TIMEOUT))
        .map_err(|error| format!("read timeout could not be set: {error}"))?;
    stream
        .set_write_timeout(Some(API_TIMEOUT))
        .map_err(|error| format!("write timeout could not be set: {error}"))?;
    let envelope = json!({
        "id": format!("herdr-core:{method}"),
        "method": method,
        "params": params,
    });
    let mut request_line = serde_json::to_vec(&envelope)
        .map_err(|error| format!("request could not be encoded: {error}"))?;
    request_line.push(b'\n');
    stream
        .write_all(&request_line)
        .map_err(|error| format!("request could not be written: {error}"))?;

    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|error| format!("response could not be read: {error}"))?;
    if line.trim().is_empty() {
        return Err("response was empty".to_owned());
    }
    let response: Value = serde_json::from_str(&line)
        .map_err(|error| format!("response was not valid JSON: {error}"))?;
    if let Some(error) = response.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("herdr request failed");
        return Err(format!("{method} failed with {code}: {message}"));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| format!("{method} response is missing result"))
}

/// A live byte transport to one herdr pane: `herdr pane attach <pane_id>`
/// running under a local PTY. Dropping it kills the attach client.
pub struct PaneAttach {
    pub pane_id: String,
    pub generation: u64,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
}

impl PaneAttach {
    pub fn spawn(
        context: &LiveContext,
        pane_id: &str,
        generation: u64,
        rows: u16,
        cols: u16,
    ) -> Result<Self, String> {
        let Some(herdr_bin) = context.herdr_bin.as_ref() else {
            return Err(
                "herdr binary was not found; install herdr or set its path in the app options"
                    .to_owned(),
            );
        };
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("PTY could not be opened: {error}"))?;
        let mut command = CommandBuilder::new(herdr_bin);
        command.arg("pane");
        command.arg("attach");
        command.arg(pane_id);
        command.env("HERDR_SOCKET_PATH", &context.socket_path);
        command.env("TERM", "xterm-256color");
        command.env("LANG", "en_US.UTF-8");
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("herdr pane attach could not be spawned: {error}"))?;
        drop(pair.slave);
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("PTY writer could not be taken: {error}"))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("PTY reader could not be cloned: {error}"))?;

        let runtime = context.runtime.clone();
        let notifier = context.notifier.clone();
        let reader_pane = pane_id.to_owned();
        thread::Builder::new()
            .name(format!("herdr-core-attach-{pane_id}"))
            .spawn(move || {
                let mut bytes = [0_u8; 8192];
                loop {
                    match reader.read(&mut bytes) {
                        Ok(0) => {
                            deliver_attach_exit(&runtime, &notifier, generation, &reader_pane);
                            return;
                        }
                        Ok(count) => {
                            if !deliver_attach_output(
                                &runtime,
                                &notifier,
                                generation,
                                &bytes[..count],
                            ) {
                                return;
                            }
                        }
                        Err(error) => {
                            deliver_attach_error(
                                &runtime,
                                &notifier,
                                generation,
                                &reader_pane,
                                &error.to_string(),
                            );
                            return;
                        }
                    }
                }
            })
            .map_err(|error| format!("attach reader thread could not be started: {error}"))?;

        Ok(Self {
            pane_id: pane_id.to_owned(),
            generation,
            master: pair.master,
            child,
            writer,
        })
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.writer
            .write_all(bytes)
            .and_then(|()| self.writer.flush())
            .map_err(|error| format!("pane input could not be written: {error}"))
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<(), String> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("pane could not be resized: {error}"))
    }
}

impl Drop for PaneAttach {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn deliver_attach_output(
    runtime: &Weak<Mutex<Runtime>>,
    notifier: &ChangeNotifier,
    generation: u64,
    bytes: &[u8],
) -> bool {
    let Some(runtime) = runtime.upgrade() else {
        return false;
    };
    let delivered = match runtime.lock() {
        Ok(mut guard) => guard.ingest_attach_output(generation, bytes),
        Err(_) => return false,
    };
    drop(runtime);
    if delivered {
        notifier.notify();
    }
    delivered
}

fn deliver_attach_exit(
    runtime: &Weak<Mutex<Runtime>>,
    notifier: &ChangeNotifier,
    generation: u64,
    pane_id: &str,
) {
    let Some(runtime) = runtime.upgrade() else {
        return;
    };
    let delivered = match runtime.lock() {
        Ok(mut guard) => guard.ingest_attach_exit(
            generation,
            format!("Pane {pane_id} attach ended; it may be attached elsewhere or closed"),
        ),
        Err(_) => return,
    };
    drop(runtime);
    if delivered {
        notifier.notify();
    }
}

fn deliver_attach_error(
    runtime: &Weak<Mutex<Runtime>>,
    notifier: &ChangeNotifier,
    generation: u64,
    pane_id: &str,
    error: &str,
) {
    let Some(runtime) = runtime.upgrade() else {
        return;
    };
    let delivered = match runtime.lock() {
        Ok(mut guard) => {
            guard.ingest_attach_exit(generation, format!("Pane {pane_id} stream failed: {error}"))
        }
        Err(_) => return,
    };
    drop(runtime);
    if delivered {
        notifier.notify();
    }
}

pub fn encode_base64(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

pub fn decode_base64(value: &str) -> Result<Vec<u8>, String> {
    BASE64
        .decode(value)
        .map_err(|error| format!("base64 payload could not be decoded: {error}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wire_snapshot_projects_agents_with_workspace_labels_and_verbatim_tokens() {
        let snapshot = json!({
            "protocol": HERDR_PROTOCOL_REVISION,
            "workspaces": [
                {"workspace_id": "w1", "label": "herdr-ide"},
            ],
            "agents": [
                {
                    "pane_id": "w1:p1",
                    "workspace_id": "w1",
                    "agent": "claude",
                    "agent_status": "working",
                    "cwd": "/tmp/project",
                    "tokens": {
                        "status_working": "●",
                        "sort_rank": "04",
                        "activity": "1787963036671",
                        "summary": "doing things",
                        "elapsed": "6h"
                    }
                },
                {
                    "pane_id": "w9:p2",
                    "workspace_id": "w9",
                    "tokens": {"status_idle": "○", "sort_rank": "10", "activity": "1787963036672"}
                }
            ]
        });
        let payload = project_session(&snapshot).expect("projects");
        assert_eq!(payload.agents.len(), 2);
        assert_eq!(payload.agents[0].pane_id.as_deref(), Some("w1:p1"));
        assert_eq!(
            payload.agents[0].workspace_label.as_deref(),
            Some("herdr-ide")
        );
        assert_eq!(
            payload.agents[0].tokens.get("status_working"),
            Some(&json!("●"))
        );
        // A workspace without a label entry falls back to its id.
        assert_eq!(payload.agents[1].workspace_label.as_deref(), Some("w9"));

        let projected = crate::sidebar::project_agents(payload).expect("sidebar projection");
        assert_eq!(projected[0].state, "working");
        assert_eq!(projected[1].state, "idle");
    }

    #[test]
    fn protocol_mismatch_is_an_explicit_failure() {
        let snapshot = json!({"protocol": 20, "workspaces": [], "agents": []});
        let error = project_session(&snapshot).expect_err("must fail");
        assert_eq!(error.state(), "protocol_mismatch");
        assert!(error.message().contains("20"));
    }

    #[test]
    fn missing_socket_file_is_distinguished_from_unreachable() {
        let error =
            fetch_session(Path::new("/nonexistent/herdr-core-test.sock")).expect_err("must fail");
        assert_eq!(error.state(), "socket_missing");
    }
}
