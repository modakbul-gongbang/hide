//! A pane's attach child never outlives what started it (PRD
//! core-host-node-terminal B20, D-20): a `hided` killed with `SIGKILL` leaves
//! no `herdr terminal session` behind, and a device whose node link drops
//! ends the sessions its node started. Each child's stdin is a pipe from the
//! process that started it, and that pipe closing, however the process ends,
//! is what ends the child.
//!
//! Real processes against the pinned Herdr, so the tests are ignored by
//! default; run them with `--ignored` and the binary in `HIDE_E2E_HERDR_BIN`.
//! The Herdr server is private: its own socket, session, config and home.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use hide_node_link::protocol::{Call, Request};
use hide_node_link::terminal::{GridSize, TerminalControl, TerminalDown, TerminalLine};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::ORIGIN;

/// How long a child may take to see its stdin close and exit.
const CHILD_EXIT: Duration = Duration::from_secs(15);

fn without_inherited_panes(command: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars_os() {
        let upper = key.to_string_lossy().to_uppercase();
        if upper.starts_with("HERDR_") || upper.starts_with("HIDE_") {
            command.env_remove(key);
        }
    }
    command
}

/// A Herdr server of the test's own, stopped on drop.
struct PrivateHerdr {
    bin: PathBuf,
    home: PathBuf,
    socket: PathBuf,
    config: PathBuf,
    server: Child,
}

impl PrivateHerdr {
    #[allow(clippy::disallowed_methods)] // polls the starting server's ping, bounded by a deadline
    fn start(root: &Path) -> Self {
        let bin = PathBuf::from(
            std::env::var_os("HIDE_E2E_HERDR_BIN")
                .expect("HIDE_E2E_HERDR_BIN names the pinned herdr"),
        );
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let config = root.join("herdr-config.toml");
        std::fs::write(
            &config,
            "[update]\nversion_check = false\nmanifest_check = false\n",
        )
        .unwrap();
        let socket = root.join("herdr.sock");
        let mut herdr = Self {
            server: Command::new("/usr/bin/true").spawn().unwrap(),
            bin,
            home,
            socket,
            config,
        };
        herdr.server = herdr
            .command()
            .arg("server")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("herdr server starts");
        let deadline = Instant::now() + Duration::from_secs(60);
        while hide_herdr_client::request_with_timeout(
            &herdr.socket,
            "ping",
            json!({}),
            Duration::from_secs(2),
        )
        .is_err()
        {
            assert!(
                Instant::now() < deadline,
                "the private Herdr never answered"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        herdr
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.bin);
        without_inherited_panes(&mut command)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join("xdg-config"))
            .env("XDG_STATE_HOME", self.home.join("xdg-state"))
            .env("HERDR_SESSION", "hide-terminal-children")
            .env("HERDR_SOCKET_PATH", &self.socket)
            .env("HERDR_CONFIG_PATH", &self.config)
            .env("HERDR_DISABLE_SOUND", "1");
        command
    }

    /// A new workspace's first pane.
    fn pane(&self) -> String {
        let project = self.home.join("proof");
        std::fs::create_dir_all(&project).unwrap();
        let created = hide_herdr_client::request(
            &self.socket,
            "workspace.create",
            json!({"cwd": project.display().to_string(), "label": "proof"}),
        )
        .expect("workspace.create");
        first_pane(&created).unwrap_or_else(|| panic!("no pane in {created}"))
    }
}

impl Drop for PrivateHerdr {
    fn drop(&mut self) {
        let _ = self
            .command()
            .args(["server", "stop"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

fn first_pane(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => map
            .get("pane_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| map.values().find_map(first_pane)),
        Value::Array(items) => items.iter().find_map(first_pane),
        _ => None,
    }
}

/// The `herdr terminal session` processes whose parent is `parent`.
fn session_children(parent: u32) -> Vec<u32> {
    let listing = Command::new("/bin/ps")
        .args(["-A", "-o", "pid=,ppid=,command="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&listing.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let ppid = fields.next()?.parse::<u32>().ok()?;
            let command = fields.collect::<Vec<_>>().join(" ");
            (ppid == parent && command.contains("terminal session")).then_some(pid)
        })
        .collect()
}

/// Whether `pid` is a live process (not gone, not a zombie).
fn alive(pid: u32) -> bool {
    let status = Command::new("/bin/ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("ps");
    let stat = String::from_utf8_lossy(&status.stdout);
    let stat = stat.trim();
    !stat.is_empty() && !stat.starts_with('Z')
}

#[allow(clippy::disallowed_methods)] // polls a process table, bounded by a deadline
fn wait_for<T>(within: Duration, mut probe: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + within;
    loop {
        if let Some(found) = probe() {
            return Some(found);
        }
        if Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn kill(pid: u32) {
    let status = Command::new("/bin/kill")
        .args(["-KILL", &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success(), "kill -KILL {pid}");
}

#[test]
#[ignore = "needs the pinned Herdr: set HIDE_E2E_HERDR_BIN and run with --ignored"]
fn a_killed_daemon_leaves_no_attach_child() {
    // A Unix socket path is limited to about a hundred bytes.
    let root = tempfile::Builder::new()
        .prefix("htc")
        .tempdir_in("/tmp")
        .unwrap();
    let herdr = PrivateHerdr::start(root.path());
    let state = root.path().join("state");
    let mut command = Command::new(env!("CARGO_BIN_EXE_hided"));
    without_inherited_panes(&mut command)
        .env("HOME", &herdr.home)
        .env("HIDE_STATE_DIR", &state)
        .env("HIDE_KEEP_ALIVE", "1")
        .env("HIDE_TAILSCALE_BIN", root.path().join("no-tailscale"))
        .env("HERDR_SOCKET_PATH", &herdr.socket)
        .env("HERDR_BIN_PATH", &herdr.bin)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut daemon = command.spawn().expect("hided starts");
    let started = wait_for(Duration::from_secs(60), || {
        let bytes = std::fs::read(state.join("hided.json")).ok()?;
        let value: Value = serde_json::from_slice(&bytes).ok()?;
        Some((value["port"].as_u64()?, value["token"].as_str()?.to_owned()))
    });
    let Some((port, token)) = started else {
        let _ = daemon.kill();
        panic!("hided never wrote its state");
    };
    let project = herdr.home.join("proof");
    std::fs::create_dir_all(&project).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let socket = runtime.block_on(async {
        let mut request = format!("ws://127.0.0.1:{port}/ws")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert(ORIGIN, format!("http://127.0.0.1:{port}").parse().unwrap());
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let send = |value: Value| Message::Text(value.to_string().into());
        socket
            .send(send(json!({"token": token, "schema_version": 2})))
            .await
            .unwrap();
        socket
            .send(send(json!({"schema_version": 2, "kind": "create_workspace", "payload": {
                "path": project.display().to_string(), "label": "proof", "initialize_git": false,
            }})))
            .await
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let pane = loop {
            let left = deadline
                .checked_duration_since(Instant::now())
                .expect("the new Workspace's pane is focused within a minute");
            let Ok(Some(Ok(Message::Text(text)))) = tokio::time::timeout(left, socket.next()).await
            else {
                continue;
            };
            let frame: Value = serde_json::from_str(&text).unwrap();
            if let Some(pane) = frame["payload"]["rest"]["focused"]["pane_id"].as_str() {
                break pane.to_owned();
            }
        };
        for kind in ["terminal_viewport", "terminal_resize"] {
            socket
                .send(send(json!({"schema_version": 2, "kind": kind, "payload": {
                    "pane_id": pane, "cols": 100, "rows": 30, "new_view": true,
                }})))
                .await
                .unwrap();
        }
        socket
    });
    let children = wait_for(Duration::from_secs(60), || {
        let children = session_children(daemon.id());
        (!children.is_empty()).then_some(children)
    })
    .expect("hided attached the pane through a terminal session child");
    kill(daemon.id());
    daemon.wait().unwrap();
    drop(socket);
    let left = wait_for(CHILD_EXIT, || {
        let left = children
            .iter()
            .copied()
            .filter(|pid| alive(*pid))
            .collect::<Vec<_>>();
        left.is_empty().then_some(())
    });
    if left.is_none() {
        for pid in &children {
            let _ = Command::new("/bin/kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }
        panic!("terminal session children {children:?} outlived a SIGKILLed hided");
    }
}

#[test]
#[ignore = "needs the pinned Herdr: set HIDE_E2E_HERDR_BIN and run with --ignored"]
fn a_dropped_device_link_ends_the_devices_attach_children() {
    let root = tempfile::Builder::new()
        .prefix("htc")
        .tempdir_in("/tmp")
        .unwrap();
    let herdr = PrivateHerdr::start(root.path());
    let pane = herdr.pane();
    // The device's node finds `herdr` where an installer puts it.
    let bin = herdr.home.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(&herdr.bin, bin.join("herdr")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hided"));
    without_inherited_panes(&mut command)
        .args(["node", "serve"])
        .env("HOME", &herdr.home)
        .env("SHELL", "/bin/sh")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut node = command.spawn().expect("the device's node starts");
    let mut link = node.stdin.take().unwrap();
    let mut answers = BufReader::new(node.stdout.take().unwrap());
    let start = Request {
        id: 1,
        call: Call::TerminalsStart {
            herdr_socket: herdr.socket.display().to_string(),
        },
    };
    writeln!(link, "{}", serde_json::to_string(&start).unwrap()).unwrap();
    let mut line = String::new();
    answers.read_line(&mut line).unwrap();
    let answer: Value = serde_json::from_str(&line).unwrap();
    assert!(answer.get("ok").is_some(), "terminals_start: {answer}");
    let attach = TerminalLine {
        terminal: TerminalDown::Control {
            control: TerminalControl::Attach {
                pane: pane.clone(),
                size: Some(GridSize {
                    rows: 30,
                    cols: 100,
                }),
                manual: false,
            },
        },
    };
    writeln!(link, "{}", serde_json::to_string(&attach).unwrap()).unwrap();
    // The link's reader keeps draining what the node sends, as a link does.
    let drain = std::thread::spawn(move || {
        let mut sink = String::new();
        while answers.read_line(&mut sink).is_ok_and(|read| read > 0) {
            sink.clear();
        }
    });
    let children = wait_for(Duration::from_secs(60), || {
        let children = session_children(node.id());
        (!children.is_empty()).then_some(children)
    });
    let Some(children) = children else {
        let _ = node.kill();
        panic!("the node never attached {pane}");
    };
    // The link drops: the node's input ends, as an SSH channel's does.
    drop(link);
    let ended = wait_for(CHILD_EXIT, || node.try_wait().unwrap());
    if ended.is_none() {
        let _ = node.kill();
    }
    let left = wait_for(CHILD_EXIT, || {
        children.iter().all(|pid| !alive(*pid)).then_some(())
    });
    if left.is_none() {
        for pid in &children {
            let _ = Command::new("/bin/kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }
        panic!("terminal session children {children:?} outlived the dropped link");
    }
    assert!(ended.is_some(), "the node outlived its link");
    drain.join().unwrap();
    // The pane itself is Herdr's and outlives the attach.
    let listed = hide_herdr_client::request(&herdr.socket, "pane.get", json!({"pane_id": pane}));
    assert!(listed.is_ok(), "the pane is still Herdr's: {listed:?}");
}
