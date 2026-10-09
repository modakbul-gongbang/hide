//! A node whose core runs on another machine (PRD core-host-node-remote-core):
//! candidate hided on both sides, the pinned Herdr on each machine, and a
//! real SSH connection between them that only the screen machine opens.
#![cfg(unix)]

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use hided::node_role::{NodeIdentity, NodeRole, Phase};
use serde_json::Value;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::support::remote_core::{CORE_NODE, Fixture};
use crate::support::remote_delivery::wait_for;

const LINK_BOUND: Duration = Duration::from_secs(30);

/// Where an in-process node's own panes' output goes: a hub no screen reads.
fn screen() -> std::sync::Arc<dyn hide_node::terminal::OutputSink> {
    hided::terminal_hub::TerminalHub::new()
}

fn identity(fixture: &Fixture) -> Result<NodeIdentity> {
    Ok(NodeIdentity {
        node: herdr_core::node::NodeId::of_this_machine()
            .map_err(anyhow::Error::msg)?
            .as_str()
            .to_owned(),
        label: "screen-fixture".to_owned(),
        build: hided::build_id::of_file(&fixture.hided).map_err(anyhow::Error::msg)?,
        herdr_socket: fixture.screen.socket.clone(),
        herdr_bin: std::env::var_os("HIDE_E2E_HERDR_BIN")
            .map(std::path::PathBuf::from)
            .context("HIDE_E2E_HERDR_BIN")?,
    })
}

#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_node_dials_its_core_and_the_core_reaches_its_herdr_through_the_link() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let identity = identity(&fixture)?;
        let node = identity.node.clone();
        let role = NodeRole::start(
            fixture.screen_home(),
            fixture.placement(),
            identity,
            screen(),
        )
        .map_err(anyhow::Error::msg)?;
        let phase = role.wait_for(LINK_BOUND, |phase| matches!(phase, Phase::Live(_)));
        ensure!(
            matches!(phase, Phase::Live(_)),
            "the node never linked: {phase:?}"
        );
        // The core took the node as a device of its own, keyed by the node
        // id, with no consent asked and its helper ready.
        let row = wait_for("the node's ready row on the core", || {
            Ok(fixture.device(&node)?.filter(|row| {
                row.pointer("/host/state")
                    .is_some_and(|state| state == "ready")
            }))
        })?;
        ensure!(
            row["kind"] == "remote",
            "the node's row is not remote: {row}"
        );
        // The core reaches the node's Herdr only through the link: a
        // checkout on the node registers and its session reads.
        let project = fixture.screen_home().join("project");
        fixture.create_workspace_on(&node, &project)?;
        wait_for("the node's checkout registered on the core", || {
            let snapshot = fixture.snapshot()?;
            Ok(snapshot
                .pointer("/status/remote")
                .and_then(Value::as_array)
                .and_then(|rows| rows.iter().find(|row| row["target_id"] == node.as_str()))
                .and_then(|remote| remote.pointer("/session/workspaces"))
                .and_then(Value::as_array)
                .is_some_and(|rows| {
                    rows.iter().any(|row| {
                        row["registered"] == true
                            && row["path"] == project.to_string_lossy().as_ref()
                    })
                })
                .then_some(()))
        })?;
        ensure!(
            !fixture.core_log("node_link", "attach.linked")?.is_empty(),
            "the core logged no linked node"
        );
        // The node's end ends the link, and the core's row says so.
        drop(role);
        wait_for("the core's row after the node left", || {
            Ok(fixture
                .device(&node)?
                .filter(|row| {
                    row.pointer("/host/state")
                        .is_some_and(|state| state != "ready")
                })
                .map(|_| ()))
        })?;
        Ok(())
    })();
    match journey {
        Ok(()) => fixture.remove_run_dir(),
        Err(error) => {
            let _ = fixture.stop();
            Err(error).context(format!("run kept at {}", fixture.root.display()))
        }
    }
}

#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_node_of_another_build_or_the_cores_own_machine_is_refused() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let mut other = identity(&fixture)?;
        other.build = "0".repeat(64);
        let role = NodeRole::start(fixture.screen_home(), fixture.placement(), other, screen())
            .map_err(anyhow::Error::msg)?;
        let phase = role.wait_for(LINK_BOUND, |phase| matches!(phase, Phase::Waiting { .. }));
        ensure!(
            phase
                == Phase::Waiting {
                    reason: "other_build".to_owned()
                },
            "another build was not refused: {phase:?}"
        );
        drop(role);
        let mut own = identity(&fixture)?;
        own.node = CORE_NODE.to_owned();
        let role = NodeRole::start(fixture.screen_home(), fixture.placement(), own, screen())
            .map_err(anyhow::Error::msg)?;
        let phase = role.wait_for(LINK_BOUND, |phase| matches!(phase, Phase::Waiting { .. }));
        ensure!(
            phase
                == Phase::Waiting {
                    reason: "own_node".to_owned()
                },
            "the core's own machine was not refused: {phase:?}"
        );
        Ok(())
    })();
    match journey {
        Ok(()) => fixture.remove_run_dir(),
        Err(error) => {
            let _ = fixture.stop();
            Err(error).context(format!("run kept at {}", fixture.root.display()))
        }
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A screen of the node's machine: the node's `/ws`, as the web shell opens it.
async fn screen_socket(port: u16, token: &str) -> Result<Socket> {
    let mut request = format!("ws://127.0.0.1:{port}/ws").into_client_request()?;
    request
        .headers_mut()
        .insert("origin", format!("http://127.0.0.1:{port}").parse()?);
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await?;
    socket
        .send(Message::Text(
            json!({"token": token, "schema_version": 2, "client_kind": "web"})
                .to_string()
                .into(),
        ))
        .await?;
    Ok(socket)
}

async fn send(socket: &mut Socket, kind: &str, payload: Value) -> Result<()> {
    socket
        .send(Message::Text(
            json!({"schema_version": 2, "kind": kind, "payload": payload})
                .to_string()
                .into(),
        ))
        .await?;
    Ok(())
}

/// The next text frame, or `None` once the deadline passed.
async fn next_frame(socket: &mut Socket, deadline: Instant) -> Result<Option<Value>> {
    loop {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return Ok(None);
        };
        let Ok(message) = tokio::time::timeout(left, socket.next()).await else {
            return Ok(None);
        };
        match message {
            Some(Ok(Message::Text(text))) => return Ok(Some(serde_json::from_str(&text)?)),
            Some(Ok(Message::Close(frame))) => bail!("the screen socket closed: {frame:?}"),
            Some(Ok(_)) => {}
            Some(Err(error)) => bail!("the screen socket failed: {error}"),
            None => bail!("the screen socket ended"),
        }
    }
}

/// Every `{pane_id, bytes_base64}` chunk of a `terminal` frame, decoded.
fn chunks(frame: &Value) -> Vec<(String, Vec<u8>)> {
    if frame["type"] != "terminal" {
        return Vec::new();
    }
    frame["payload"]["chunks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|chunk| {
            let pane = chunk["pane_id"].as_str()?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(chunk["bytes_base64"].as_str()?)
                .ok()?;
            Some((pane.to_owned(), bytes))
        })
        .collect()
}

/// The screen's text without its control sequences.
fn plain(screen: &str) -> String {
    let mut text = String::new();
    let mut chars = screen.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if !c.is_control() || c == '\n' {
                text.push(c);
            }
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    text
}

async fn node_health(port: u16) -> Result<Value> {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream
        .write_all(
            format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let text = String::from_utf8(response)?;
    let body = text.split_once("\r\n\r\n").context("health body")?.1;
    Ok(serde_json::from_str(body)?)
}

/// Types `line` into `pane` once its prompt drew, and reads the screen
/// until `answer` appears on it.
async fn type_and_read(socket: &mut Socket, pane: &str, line: &str, answer: &str) -> Result<()> {
    for kind in ["terminal_viewport", "terminal_resize"] {
        send(
            socket,
            kind,
            json!({"pane_id": pane, "cols": 100, "rows": 30, "new_view": true}),
        )
        .await?;
    }
    let mut screen = String::new();
    let mut typed = false;
    let deadline = Instant::now() + Duration::from_secs(60);
    while let Some(frame) = next_frame(socket, deadline).await? {
        for (from, bytes) in chunks(&frame) {
            if from == pane {
                screen.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        if !typed && !plain(&screen).trim().is_empty() {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let keys = base64::engine::general_purpose::STANDARD.encode(format!("{line}\r"));
            send(
                socket,
                "key",
                json!({"pane_id": pane, "bytes_base64": keys}),
            )
            .await?;
            typed = true;
            screen.clear();
        }
        if typed && plain(&screen).contains(answer) {
            return Ok(());
        }
    }
    bail!(
        "{answer} never came back on {pane} (typed: {typed}); the screen read: {:?}",
        plain(&screen)
    )
}

#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_screen_on_the_node_draws_the_cores_state_and_its_own_pane() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let (port, token) = fixture.start_node()?;
        let node = herdr_core::node::NodeId::of_this_machine()
            .map_err(anyhow::Error::msg)?
            .as_str()
            .to_owned();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            let deadline = Instant::now() + LINK_BOUND;
            loop {
                let health = node_health(port).await?;
                ensure!(health["role"] == "node", "the daemon is not in the node role: {health}");
                if health["core_link"] == "live" {
                    break;
                }
                ensure!(Instant::now() < deadline, "the node never linked: {health}");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            ensure!(
                !fixture.node_log("node_daemon", "started")?.is_empty(),
                "the node role logged no start beside its state"
            );
            let mut socket = screen_socket(port, &token).await?;
            // The screen's first state is the core's: the node is one of its
            // devices.
            let deadline = Instant::now() + Duration::from_secs(20);
            let snapshot = loop {
                let frame = next_frame(&mut socket, deadline)
                    .await?
                    .context("no snapshot reached the node's screen")?;
                if frame["type"] == "snapshot" {
                    break frame;
                }
            };
            let devices = snapshot["payload"]["rest"]["navigator"]["devices"].clone();
            ensure!(
                devices
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|row| row["id"] == node.as_str())),
                "the core's snapshot did not reach the node's screen: {devices}"
            );
            // A checkout on the node, registered from the node's screen: its
            // pane's keys and output stay on the node.
            let project = fixture.screen_home().join("project");
            let herdr_pane = fixture.screen.workspace_at(&project)?;
            let pane = format!("remote:{node}:pane:{herdr_pane}");
            send(
                &mut socket,
                "create_workspace",
                json!({"device_id": node, "path": project, "label": "node", "initialize_git": false}),
            )
            .await?;
            send(&mut socket, "focus_device", json!({"device_id": node})).await?;
            type_and_read(&mut socket, &pane, "echo node-\"ok\"", "node-ok").await?;
            Ok::<_, anyhow::Error>(())
        })
    })();
    match journey {
        Ok(()) => fixture.remove_run_dir(),
        Err(error) => {
            let _ = fixture.stop();
            Err(error).context(format!("run kept at {}", fixture.root.display()))
        }
    }
}
