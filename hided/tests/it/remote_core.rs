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

/// Brings the checkout at `path` on `device` forward from `socket`, as a
/// click on its sidebar row does, once the core's navigator lists it.
async fn focus_checkout(
    fixture: &Fixture,
    socket: &mut Socket,
    device: &str,
    path: &std::path::Path,
) -> Result<()> {
    send(socket, "focus_device", json!({"device_id": device})).await?;
    let path = path.to_string_lossy().into_owned();
    let mut seen = Value::Null;
    let (workspace, checkout) = tokio::task::block_in_place(|| {
        wait_for("the checkout in the core's navigator", || {
            let snapshot = fixture.snapshot()?;
            // This machine's checkouts are the navigator's; a device's are
            // its session's.
            let mut listed: Vec<Value> = snapshot["navigator"]["workspaces"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            for remote in snapshot["status"]["remote"]
                .as_array()
                .into_iter()
                .flatten()
            {
                listed.extend(
                    remote["session"]["workspaces"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                );
            }
            seen = json!(
                listed
                    .iter()
                    .map(|workspace| &workspace["path"])
                    .collect::<Vec<_>>()
            );
            Ok(listed
                .iter()
                .find(|workspace| workspace["path"] == path.as_str())
                .and_then(|workspace| {
                    Some((
                        workspace["id"].as_str()?.to_owned(),
                        workspace["checkouts"][0]["id"].as_str()?.to_owned(),
                    ))
                }))
        })
    })
    .with_context(|| format!("the core's navigator: {seen}"))?;
    send(
        socket,
        "focus_checkout",
        json!({"workspace_id": workspace, "checkout_id": checkout, "focus_device": true}),
    )
    .await
}

/// Waits up to `bound` for the node's link to reach `phase`, as its
/// `/health` reports it, and answers that report.
async fn node_link(port: u16, phase: &str, bound: Duration) -> Result<Value> {
    let deadline = Instant::now() + bound;
    loop {
        let health = node_health(port).await?;
        ensure!(
            health["role"] == "node",
            "the daemon is not in the node role: {health}"
        );
        if health["core_link"] == phase {
            return Ok(health);
        }
        ensure!(
            Instant::now() < deadline,
            "the node's link never reached {phase}: {health}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The first snapshot frame `socket` receives within `bound`.
async fn first_snapshot(socket: &mut Socket, bound: Duration) -> Result<Value> {
    let deadline = Instant::now() + bound;
    loop {
        let frame = next_frame(socket, deadline)
            .await?
            .context("no snapshot reached the screen")?;
        if frame["type"] == "snapshot" {
            return Ok(frame);
        }
    }
}

/// Reads `socket` until it closes within `bound`, and answers the close
/// frame's code and reason; a frame that is not a close is skipped.
async fn close_of(socket: &mut Socket, bound: Duration) -> Result<(u16, String)> {
    let deadline = Instant::now() + bound;
    loop {
        let left = deadline
            .checked_duration_since(Instant::now())
            .context("the screen socket never closed")?;
        let message = tokio::time::timeout(left, socket.next())
            .await
            .context("the screen socket never closed")?;
        match message {
            Some(Ok(Message::Close(Some(frame)))) => {
                return Ok((u16::from(frame.code), frame.reason.to_string()));
            }
            Some(Ok(Message::Close(None))) | None => bail!("the screen closed with no frame"),
            Some(Ok(_)) => {}
            Some(Err(error)) => bail!("the screen socket failed before its close: {error}"),
        }
    }
}

/// Everything `pane` draws on `socket` for `bound` after a new view asks
/// it whole, without its control sequences.
async fn pane_text(socket: &mut Socket, pane: &str, bound: Duration) -> Result<String> {
    send(
        socket,
        "terminal_viewport",
        json!({"pane_id": pane, "cols": 100, "rows": 30, "new_view": true}),
    )
    .await?;
    let mut screen = String::new();
    let deadline = Instant::now() + bound;
    while let Some(frame) = next_frame(socket, deadline).await? {
        for (from, bytes) in chunks(&frame) {
            if from == pane {
                screen.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
    }
    Ok(plain(&screen))
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
/// until `answer` appears on it; answers what the pane drew after the
/// keys, without its control sequences.
async fn type_and_read(
    socket: &mut Socket,
    pane: &str,
    line: &str,
    answer: &str,
) -> Result<String> {
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
            return Ok(plain(&screen));
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
            node_link(port, "live", LINK_BOUND).await?;
            ensure!(
                !fixture.node_log("node_daemon", "started")?.is_empty(),
                "the node role logged no start beside its state"
            );
            let mut socket = screen_socket(port, &token).await?;
            // The screen's first state is the core's: the node is one of its
            // devices.
            let snapshot = first_snapshot(&mut socket, Duration::from_secs(20)).await?;
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
            focus_checkout(&fixture, &mut socket, &node, &project).await?;
            type_and_read(&mut socket, &pane, "echo node-\"ok\"", "node-ok").await?;
            // A pane of the core's machine, typed from the node's screen: its
            // keys and output ride the node's terminals relay.
            let core_project = fixture.core_home().join("project");
            let core_pane = fixture.core.workspace_at(&core_project)?;
            send(
                &mut socket,
                "create_workspace",
                json!({"path": core_project, "label": "core", "initialize_git": false}),
            )
            .await?;
            focus_checkout(&fixture, &mut socket, CORE_NODE, &core_project).await?;
            type_and_read(&mut socket, &core_pane, "echo core-\"ok\"", "core-ok").await?;
            // A window of the core's own draws the node's pane too, once it
            // opens: the node sends its output up only then.
            let (core_port, core_token) = fixture.core_screen();
            let mut window = screen_socket(core_port, &core_token).await?;
            let deadline = Instant::now() + Duration::from_secs(20);
            while next_frame(&mut window, deadline)
                .await?
                .context("no snapshot reached the core's window")?["type"]
                != "snapshot"
            {}
            // The operator looks at the node's checkout again, from the
            // node's screen, and a window of the core's own opens on it.
            focus_checkout(&fixture, &mut socket, &node, &project).await?;
            send(
                &mut window,
                "terminal_viewport",
                json!({"pane_id": pane, "cols": 100, "rows": 30, "new_view": true}),
            )
            .await?;
            let keys = base64::engine::general_purpose::STANDARD.encode("echo mirror-\"ok\"\r");
            send(&mut socket, "key", json!({"pane_id": pane, "bytes_base64": keys})).await?;
            let mut drawn = String::new();
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let frame = next_frame(&mut window, deadline).await?.with_context(|| {
                    format!("the node's pane never drew on the core's window: {:?}", plain(&drawn))
                })?;
                for (from, bytes) in chunks(&frame) {
                    if from == pane {
                        drawn.push_str(&String::from_utf8_lossy(&bytes));
                    }
                }
                if plain(&drawn).contains("mirror-ok") {
                    break;
                }
            }
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

/// What a screen shows of the operator's layout: the focus, the tab and
/// the panes' places, as one snapshot frame carries them.
fn layout(snapshot: &Value) -> Value {
    let rest = &snapshot["payload"]["rest"];
    json!({
        "focused": rest["focused"],
        "tab": rest["tab"],
        "zoomed": rest["zoomed"],
        "pane_layouts": rest["pane_layouts"],
    })
}

#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_node_that_loses_its_core_holds_its_screens_and_returns_as_it_was() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let (port, token) = fixture.start_node()?;
        let node = herdr_core::node::NodeId::of_this_machine()
            .map_err(anyhow::Error::msg)?
            .as_str()
            .to_owned();
        let project = fixture.screen_home().join("project");
        let herdr_pane = fixture.screen.workspace_at(&project)?;
        let pane = format!("remote:{node}:pane:{herdr_pane}");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            node_link(port, "live", LINK_BOUND).await?;
            let mut socket = screen_socket(port, &token).await?;
            first_snapshot(&mut socket, Duration::from_secs(20)).await?;
            send(
                &mut socket,
                "create_workspace",
                json!({"device_id": node, "path": project, "label": "node", "initialize_git": false}),
            )
            .await?;
            focus_checkout(&fixture, &mut socket, &node, &project).await?;
            type_and_read(&mut socket, &pane, "echo before-\"ok\"", "before-ok").await?;
            // The layout as a screen opened now draws it.
            let mut look = screen_socket(port, &token).await?;
            let before = layout(&first_snapshot(&mut look, Duration::from_secs(20)).await?);
            drop(look);

            // The core's machine stops answering SSH: the open screen is
            // closed as one whose core is lost, and the node waits.
            tokio::task::block_in_place(|| fixture.ssh.online(false))?;
            let (code, reason) = close_of(&mut socket, Duration::from_secs(15)).await?;
            ensure!(
                (code, reason.as_str()) == (1012, "core_link_lost"),
                "the screen closed as {code} {reason}"
            );
            // The link's end, then a dial that finds no SSH server.
            let deadline = Instant::now() + LINK_BOUND;
            loop {
                let waiting = node_link(port, "waiting", LINK_BOUND).await?;
                let reason = waiting["core_link_reason"].as_str().unwrap_or_default();
                if reason.starts_with("unreachable") {
                    break;
                }
                ensure!(
                    Instant::now() < deadline,
                    "the node waits for another reason: {waiting}"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            // A screen opened meanwhile is held: nothing reaches it, and the
            // keys it sends are dropped rather than kept for later.
            let mut held = screen_socket(port, &token).await?;
            let keys = base64::engine::general_purpose::STANDARD.encode("echo held-\"leak\"\r");
            send(&mut held, "key", json!({"pane_id": pane, "bytes_base64": keys})).await?;
            let quiet = next_frame(&mut held, Instant::now() + Duration::from_secs(5)).await?;
            ensure!(quiet.is_none(), "a held screen received {quiet:?}");
            // The outage lasts until the node's next try is further away than
            // the return is allowed to take: only the node noticing SSH
            // answer again brings it back in time.
            tokio::task::block_in_place(|| {
                wait_for("a retry wait past the return bound", || {
                    let ended = fixture.node_log("node_role", "link.ended")?;
                    Ok(ended
                        .iter()
                        .any(|entry| entry["retry_ms"].as_u64() >= Some(16_000))
                        .then_some(()))
                })
            })?;
            let quiet = next_frame(&mut held, Instant::now()).await?;
            ensure!(quiet.is_none(), "a held screen received {quiet:?}");

            // SSH answers again: the held screen attaches within ten seconds
            // and draws the layout it left.
            tokio::task::block_in_place(|| fixture.ssh.online(true))?;
            let back = Instant::now();
            let snapshot = first_snapshot(&mut held, Duration::from_secs(10))
                .await
                .context("the held screen did not attach within ten seconds")?;
            let returned = back.elapsed();
            let after = layout(&snapshot);
            ensure!(before == after, "the layout moved: {before} then {after}");
            type_and_read(&mut held, &pane, "echo after-\"ok\"", "after-ok").await?;
            let text = pane_text(&mut held, &pane, Duration::from_secs(3)).await?;
            ensure!(
                text.contains("after-ok") && !text.contains("held-"),
                "the pane after the outage reads {text:?}"
            );
            eprintln!("reattached {returned:?} after SSH answered again");
            Ok::<_, anyhow::Error>(())
        })?;
        // B19: the link's start, end and retries are recorded with the
        // machines they joined, and nothing typed is.
        let linked = fixture.core_log("node_link", "attach.linked")?;
        ensure!(
            linked.len() >= 2 && linked.iter().all(|row| row["node"] == node.as_str()),
            "the core's link records: {linked:?}"
        );
        let ended = fixture.node_log("node_role", "link.ended")?;
        ensure!(
            ended
                .iter()
                .all(|row| row["core"] == CORE_NODE && row["retry_ms"].is_u64()),
            "the node's link records: {ended:?}"
        );
        for typed in ["before-", "after-", "held-"] {
            ensure!(!fixture.logs_mention(typed)?, "{typed} reached a log");
        }
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

/// `hide connect --json` of the screen machine, run with `hide` at `cli`.
fn screen_connect(fixture: &Fixture, cli: &std::path::Path) -> Result<Value> {
    let output = fixture
        .screen_command(&cli.join("hide"))
        .args(["connect", "--json"])
        .output()?;
    serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "hide connect answered {:?} {:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_screen_whose_core_is_not_running_attaches_and_waits_for_it() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let core_state = fixture.core_state.join("hided.json");
        let before = std::fs::read(&core_state)?;
        fixture.stop_core()?;
        let (port, _) = fixture.start_node()?;
        let cli = fixture.hided.parent().context("the CLI folder")?.to_owned();
        let answer = screen_connect(&fixture, &cli)?;
        ensure!(answer["ok"] == true, "hide connect refused: {answer}");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let health = runtime.block_on(node_health(port))?;
        ensure!(
            health["core_link"] == "waiting" && health["core_link_reason"] == "no_core",
            "the node does not wait for its core: {health}"
        );
        // The attach role answered without starting a core in its place.
        ensure!(
            std::fs::read(&core_state)? == before,
            "a core started on the core's machine"
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

#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_screen_of_another_build_than_its_core_is_told_so() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        // The screen machine's app is another build: its hided differs from
        // the core's by one byte past the program's end.
        let cli = fixture.root.join("other-build");
        std::fs::create_dir_all(&cli)?;
        let source = fixture.hided.parent().context("the CLI folder")?;
        for name in ["hide", "hided"] {
            std::fs::copy(source.join(name), cli.join(name))?;
        }
        let mut bytes = std::fs::read(cli.join("hided"))?;
        bytes.push(0);
        std::fs::write(cli.join("hided"), bytes)?;
        fixture.start_node_with(&cli.join("hided"))?;
        let answer = screen_connect(&fixture, &cli)?;
        ensure!(
            answer["ok"] == false && answer["reason"] == "other_build",
            "hide connect did not answer other_build: {answer}"
        );
        let refused = fixture.core_log("node_link", "attach.refused")?;
        ensure!(
            refused
                .iter()
                .any(|row| row["reason"] == "other_build" && row["node_build"] != row["core_build"]),
            "the core logged no build mismatch: {refused:?}"
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

#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_node_pane_calls_its_core_through_the_link() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let (port, token) = fixture.start_node()?;
        let node = herdr_core::node::NodeId::of_this_machine()
            .map_err(anyhow::Error::msg)?
            .as_str()
            .to_owned();
        let project = fixture.screen_home().join("project");
        let herdr_pane = fixture.screen.workspace_at(&project)?;
        let pane = format!("remote:{node}:pane:{herdr_pane}");
        let hide = fixture.hided.with_file_name("hide");
        let call = format!(
            "HIDE_STATE_DIR={} {} workspace info; echo info-\"done\"",
            fixture.node_state().display(),
            hide.display()
        );
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            node_link(port, "live", LINK_BOUND).await?;
            let mut socket = screen_socket(port, &token).await?;
            first_snapshot(&mut socket, Duration::from_secs(20)).await?;
            send(
                &mut socket,
                "create_workspace",
                json!({"device_id": node, "path": project, "label": "node", "initialize_git": false}),
            )
            .await?;
            focus_checkout(&fixture, &mut socket, &node, &project).await?;
            // A pane's agent: vouched for by the node's kernel as that pane,
            // so it may act as the pane's agent.
            let answer = type_and_read(&mut socket, &pane, &call, "info-done").await?;
            let info = workspace_answer(&answer)?;
            ensure!(
                info["ok"] == true
                    && info["result"]["context"]["device_id"] == node.as_str()
                    && capabilities(&info).contains(&"inbox".to_owned()),
                "the pane's call was not answered as the node's pane: {info}"
            );
            // A tool outside every pane, in the node's checkout: bound to the
            // checkout, so it acts on the Workspace but not as an agent.
            let outside = |cwd: &std::path::Path, pane: Option<&str>| -> Result<Value> {
                let mut command = fixture.screen_command(&hide);
                command.args(["workspace", "info"]).current_dir(cwd);
                if let Some(pane) = pane {
                    command.env("HERDR_PANE_ID", pane);
                }
                let output = command.output()?;
                workspace_answer(&String::from_utf8_lossy(&output.stdout)).with_context(|| {
                    format!("stderr: {}", String::from_utf8_lossy(&output.stderr))
                })
            };
            let info = tokio::task::block_in_place(|| outside(&project, None))?;
            ensure!(
                info["ok"] == true
                    && info["result"]["context"]["device_id"] == node.as_str()
                    && !capabilities(&info).contains(&"inbox".to_owned()),
                "the checkout caller was not answered as the checkout: {info}"
            );
            // Naming the pane from outside it, and outside every checkout,
            // proves nothing.
            let home = fixture.screen_home().to_owned();
            let info = tokio::task::block_in_place(|| outside(&home, Some(&herdr_pane)))?;
            ensure!(
                info["ok"] == false && info["reason"] == "checkout_not_registered",
                "a caller outside the pane and every checkout was answered: {info}"
            );
            // B19: the refusal is recorded with its node and reason; no pane
            // was proven, so it names none.
            let refused = fixture.core_log("node_panes", "pane.refused")?;
            ensure!(
                refused.iter().any(|row| row["node"] == node.as_str()
                    && row["reason"] == "checkout_not_registered"
                    && row["pane_id"].is_null()),
                "the refusal records: {refused:?}"
            );
            // The link ends: the node's callers are refused at once, and
            // answered again once it is back.
            tokio::task::block_in_place(|| fixture.ssh.online(false))?;
            node_link(port, "waiting", LINK_BOUND).await?;
            let asked = Instant::now();
            let info = tokio::task::block_in_place(|| outside(&project, None))?;
            ensure!(
                info["ok"] == false && info["reason"] == "hide_unavailable",
                "a caller was answered while the link was down: {info}"
            );
            ensure!(
                asked.elapsed() < Duration::from_secs(2),
                "the refusal took {:?}",
                asked.elapsed()
            );
            tokio::task::block_in_place(|| fixture.ssh.online(true))?;
            node_link(port, "live", LINK_BOUND).await?;
            // The window reopens, as the web shell does after a close.
            let mut socket = screen_socket(port, &token).await?;
            first_snapshot(&mut socket, Duration::from_secs(20)).await?;
            let mut last = Value::Null;
            let info = tokio::task::block_in_place(|| {
                wait_for("the checkout caller answered again", || {
                    let info = outside(&project, None)?;
                    last = info.clone();
                    Ok((info["ok"] == true).then_some(info))
                })
            })
            .with_context(|| format!("the last answer: {last}"))?;
            ensure!(
                info["result"]["context"]["device_id"] == node.as_str(),
                "{info}"
            );
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

/// The one JSON answer a `hide workspace` command printed among `text`.
fn workspace_answer(text: &str) -> Result<Value> {
    let start = text.find('{').context("no answer was printed")?;
    let mut answers = serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>();
    Ok(answers.next().context("no answer was printed")??)
}

fn capabilities(info: &Value) -> Vec<String> {
    info["result"]["capabilities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|capability| capability.as_str().map(str::to_owned))
        .collect()
}

/// Sends `pane`'s grid from `socket` as a window's fit does.
async fn draw_at(socket: &mut Socket, pane: &str, cols: u16, rows: u16) -> Result<()> {
    for kind in ["terminal_viewport", "terminal_resize"] {
        send(
            socket,
            kind,
            json!({"pane_id": pane, "cols": cols, "rows": rows, "new_view": true}),
        )
        .await?;
    }
    Ok(())
}

/// Asks `pane` its terminal size from `typing`, or from `reading` when
/// none is named, and reads `reading` until the answer, marked `mark` so a
/// redraw of an earlier answer cannot pass for it; answers `rows cols`.
async fn pane_grid(
    typing: Option<&mut Socket>,
    reading: &mut Socket,
    pane: &str,
    mark: u32,
) -> Result<String> {
    // The typed line reads `G$((N))`, which only the shell turns into `GN`.
    // The answer has no spaces, which a whole redraw of the pane (another
    // window's view of it) may draw as cursor moves.
    let line = format!("echo G$(({mark}))x$(stty size | tr ' ' x)xend\r");
    let keys = base64::engine::general_purpose::STANDARD.encode(line);
    let event = json!({"pane_id": pane, "bytes_base64": keys});
    match typing {
        Some(typing) => send(typing, "key", event).await?,
        None => send(reading, "key", event).await?,
    }
    let mut screen = String::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    let marked = format!("G{mark}x");
    while let Some(frame) = next_frame(reading, deadline).await? {
        for (from, bytes) in chunks(&frame) {
            if from == pane {
                screen.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        let text = plain(&screen);
        if let Some(at) = text.find(&marked)
            && let Some((answer, _)) = text[at + marked.len()..].split_once("xend")
        {
            return Ok(answer.replace('x', " "));
        }
    }
    bail!("{pane} never answered its size: {:?}", plain(&screen))
}

#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_pane_runs_at_the_grid_of_the_screen_that_last_typed_into_it() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let (port, token) = fixture.start_node()?;
        let node = herdr_core::node::NodeId::of_this_machine()
            .map_err(anyhow::Error::msg)?
            .as_str()
            .to_owned();
        let project = fixture.screen_home().join("project");
        let herdr_pane = fixture.screen.workspace_at(&project)?;
        let pane = format!("remote:{node}:pane:{herdr_pane}");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            node_link(port, "live", LINK_BOUND).await?;
            let mut socket = screen_socket(port, &token).await?;
            first_snapshot(&mut socket, Duration::from_secs(20)).await?;
            send(
                &mut socket,
                "create_workspace",
                json!({"device_id": node, "path": project, "label": "node", "initialize_git": false}),
            )
            .await?;
            focus_checkout(&fixture, &mut socket, &node, &project).await?;
            // The screen machine's window draws the pane wide and types.
            type_and_read(&mut socket, &pane, "echo wide-\"ok\"", "wide-ok").await?;
            draw_at(&mut socket, &pane, 120, 33).await?;
            let grid = pane_grid(None, &mut socket, &pane, 1).await.context("grid 1")?;
            ensure!(grid == "33 120", "the typing window's grid: {grid}");
            // The core machine's window draws it narrow: the pane stays wide
            // while that window only looks.
            let (core_port, core_token) = fixture.core_screen();
            let mut window = screen_socket(core_port, &core_token).await?;
            first_snapshot(&mut window, Duration::from_secs(20)).await?;
            draw_at(&mut window, &pane, 70, 21).await?;
            let typed_wide = Instant::now();
            let grid = pane_grid(None, &mut socket, &pane, 2).await.context("grid 2")?;
            ensure!(grid == "33 120", "a window that only looks resized the pane: {grid}");
            // It types: the pane takes its grid.
            let grid = pane_grid(Some(&mut window), &mut socket, &pane, 3).await.context("grid 3")?;
            ensure!(grid == "21 70", "the window that typed last: {grid}");
            // The wide window types again, once its last notice of typing is
            // a second old: the pane is wide again.
            tokio::time::sleep_until((typed_wide + Duration::from_millis(1100)).into()).await;
            let grid = pane_grid(None, &mut socket, &pane, 4).await.context("grid 4")?;
            ensure!(grid == "33 120", "the wide window typed last: {grid}");
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

/// The next binary frame on `socket` within `bound`: its JSON header and
/// its bytes; text frames before it are skipped.
async fn next_bytes(socket: &mut Socket, bound: Duration) -> Result<(Value, Vec<u8>)> {
    let deadline = Instant::now() + bound;
    loop {
        let left = deadline
            .checked_duration_since(Instant::now())
            .context("no file bytes arrived")?;
        let message = tokio::time::timeout(left, socket.next())
            .await
            .context("no file bytes arrived")?;
        match message {
            Some(Ok(Message::Binary(frame))) => {
                let length = u32::from_be_bytes(frame[..4].try_into()?) as usize;
                let header = serde_json::from_slice(&frame[4..4 + length])?;
                return Ok((header, frame[4 + length..].to_vec()));
            }
            Some(Ok(Message::Text(text))) if text.contains("file_bytes") => {
                bail!("the read was refused: {text}")
            }
            Some(Ok(_)) => {}
            other => bail!("the screen socket ended: {other:?}"),
        }
    }
}

#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_node_screen_reads_its_own_checkouts_files_without_the_core() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let (port, token) = fixture.start_node()?;
        let node = herdr_core::node::NodeId::of_this_machine()
            .map_err(anyhow::Error::msg)?
            .as_str()
            .to_owned();
        let project = fixture.screen_home().join("project");
        std::fs::write(project.join("notes.txt"), "read-on-this-machine")?;
        fixture.screen.workspace_at(&project)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            node_link(port, "live", LINK_BOUND).await?;
            let mut socket = screen_socket(port, &token).await?;
            first_snapshot(&mut socket, Duration::from_secs(20)).await?;
            send(
                &mut socket,
                "create_workspace",
                json!({"device_id": node, "path": project, "label": "node", "initialize_git": false}),
            )
            .await?;
            focus_checkout(&fixture, &mut socket, &node, &project).await?;
            // The Explorer lists the checkout through the core, which opens
            // its root on this machine's node.
            send(
                &mut socket,
                "file_list",
                json!({"root": project, "path": project, "device_id": node}),
            )
            .await?;
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let frame = next_frame(&mut socket, deadline)
                    .await?
                    .context("the checkout was never listed")?;
                if frame["type"] == "directory_list" {
                    break;
                }
            }
            send(
                &mut socket,
                "file_bytes",
                json!({"request_id": "read-1", "path": project.join("notes.txt"), "device_id": node}),
            )
            .await?;
            let (header, bytes) = next_bytes(&mut socket, Duration::from_secs(20)).await?;
            ensure!(
                header["request_id"] == "read-1" && header["eof"] == true,
                "the read answered {header}"
            );
            ensure!(bytes == b"read-on-this-machine", "the file read {bytes:?}");
            // A path outside every checkout is the core's to refuse, as it
            // refuses any device's.
            send(
                &mut socket,
                "file_bytes",
                json!({"request_id": "read-2", "path": fixture.screen_home().join(".ssh/client"), "device_id": node}),
            )
            .await?;
            let refused = next_bytes(&mut socket, Duration::from_secs(20)).await;
            ensure!(refused.is_err(), "a path outside every checkout was read");
            socket.close(None).await?;
            Ok::<_, anyhow::Error>(())
        })?;
        let ended = wait_for("the screen's end on the node", || {
            let rows = fixture.node_log("node_daemon", "screen.ended")?;
            Ok((!rows.is_empty()).then_some(rows))
        })?;
        ensure!(
            ended.iter().any(|row| row["local_file_reads"] == 1),
            "the read went through the core: {ended:?}"
        );
        ensure!(
            !fixture.logs_mention("read-on-this-machine")?,
            "a file's content reached a log"
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

/// B17: a screen that stops reading while its core sends it a large file
/// holds up neither another screen nor a pane of the core's machine, and is
/// drawn again from a fresh snapshot once it reads again.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_screen_that_stops_reading_holds_up_no_other_screen() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let (port, token) = fixture.start_node()?;
        let core_project = fixture.core_home().join("project");
        let core_pane = fixture.core.workspace_at(&core_project)?;
        let big = core_project.join("big.bin");
        std::fs::write(&big, vec![b'x'; 64 * 1024 * 1024])?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            node_link(port, "live", LINK_BOUND).await?;
            let mut socket = screen_socket(port, &token).await?;
            first_snapshot(&mut socket, Duration::from_secs(20)).await?;
            send(
                &mut socket,
                "create_workspace",
                json!({"path": core_project, "label": "core", "initialize_git": false}),
            )
            .await?;
            focus_checkout(&fixture, &mut socket, CORE_NODE, &core_project).await?;
            type_and_read(&mut socket, &core_pane, "echo before-\"ok\"", "before-ok").await?;

            // A second screen asks the core for the file and reads no more.
            let mut stalled = screen_socket(port, &token).await?;
            first_snapshot(&mut stalled, Duration::from_secs(20)).await?;
            send(
                &mut stalled,
                "file_bytes",
                json!({"request_id": "big", "path": big}),
            )
            .await?;
            tokio::task::block_in_place(|| {
                wait_for("the stalled screen's backlog to be dropped", || {
                    let rows = fixture.node_log("node_daemon", "screen.fell_behind")?;
                    Ok((!rows.is_empty()).then_some(rows))
                })
            })?;
            let started = Instant::now();
            type_and_read(&mut socket, &core_pane, "echo during-\"ok\"", "during-ok").await?;
            ensure!(
                started.elapsed() < Duration::from_secs(10),
                "the core's pane answered the other screen after {:?}",
                started.elapsed()
            );

            // Reading again, the stalled screen is drawn from a whole snapshot.
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let frame = next_frame(&mut stalled, deadline)
                    .await?
                    .context("the stalled screen was never drawn again")?;
                if frame["type"] == "snapshot" {
                    break;
                }
            }
            ensure!(
                !fixture.node_log("node_daemon", "screen.resync")?.is_empty(),
                "the stalled screen's resync was not logged"
            );
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

#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_node_that_ends_leaves_no_attach_role_or_ssh_connection() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        for (signal, name) in [(libc::SIGTERM, "SIGTERM"), (libc::SIGKILL, "SIGKILL")] {
            let (port, _) = fixture.start_node()?;
            runtime.block_on(node_link(port, "live", LINK_BOUND))?;
            ensure!(
                fixture.attach_processes()?.len() == 1 && fixture.ssh.open() == 1,
                "a live node has one attach role and one connection: {:?}, {}",
                fixture.attach_processes()?,
                fixture.ssh.open()
            );
            fixture.signal_node(signal)?;
            wait_for(&format!("nothing left after {name}"), || {
                Ok(
                    (fixture.attach_processes()?.is_empty() && fixture.ssh.open() == 0)
                        .then_some(()),
                )
            })
            .with_context(|| {
                format!(
                    "left after {name}: {:?}, {} connections",
                    fixture.attach_processes(),
                    fixture.ssh.open()
                )
            })?;
        }
        // A node that stops answering but keeps its connection open: the
        // attach role ends on its own once the node fell silent.
        let (port, _) = fixture.start_node()?;
        runtime.block_on(node_link(port, "live", LINK_BOUND))?;
        fixture.signal_running_node(libc::SIGSTOP)?;
        let stopped = Instant::now();
        let ended = loop {
            if fixture.attach_processes()?.is_empty() {
                break stopped.elapsed();
            }
            ensure!(
                stopped.elapsed() < Duration::from_secs(45),
                "the attach role outlived a silent node: {:?}",
                fixture.attach_processes()?
            );
            runtime.block_on(async { tokio::time::sleep(Duration::from_millis(250)).await });
        };
        fixture.signal_running_node(libc::SIGCONT)?;
        runtime.block_on(node_link(port, "live", LINK_BOUND))?;
        eprintln!("the attach role ended {ended:?} after its node fell silent");
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
