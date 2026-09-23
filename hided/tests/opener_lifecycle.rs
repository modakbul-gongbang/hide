#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::ORIGIN;

fn fake_opener(dir: &Path) -> PathBuf {
    let script = dir.join("fake-opener");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s' \"$$\" > \"$1.pid\"\nsleep 60 &\nprintf '%s' \"$!\" > \"$1.child\"\nwait\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    script
}

fn wait_for_pid(path: &Path) -> i32 {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(value) = std::fs::read_to_string(path) {
            return value.parse().unwrap();
        }
        assert!(Instant::now() < until, "fake opener never wrote its pid");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}.{}", path.display(), suffix))
}

fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

fn assert_gone(pid: i32) {
    let until = Instant::now() + Duration::from_secs(5);
    while alive(pid) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(pid), "owned fake opener process {pid} survived");
}

#[test]
fn owner_process() {
    let Ok(marker) = std::env::var("HIDED_OWNED_OPENER_TEST_MARKER") else {
        return;
    };
    let marker = PathBuf::from(marker);
    let script = marker.parent().unwrap().join("fake-opener");
    let _opener = hided::spawn::spawn_opener(
        Path::new(env!("CARGO_BIN_EXE_hided")),
        script.as_os_str(),
        &marker,
        true,
    )
    .unwrap();
    wait_for_pid(&sidecar(&marker, "pid"));
    std::thread::sleep(Duration::from_secs(30));
}

#[test]
fn normal_close_reaps_cli_and_its_child() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_opener(dir.path());
    let marker = dir.path().join("normal");
    let mut opener = hided::spawn::spawn_opener(
        Path::new(env!("CARGO_BIN_EXE_hided")),
        script.as_os_str(),
        &marker,
        true,
    )
    .unwrap();
    let pid = wait_for_pid(&sidecar(&marker, "pid"));
    let child = wait_for_pid(&sidecar(&marker, "child"));
    opener.stop();
    assert_gone(pid);
    assert_gone(child);
}

struct TestOwner(Child);

impl Drop for TestOwner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn sigkill_of_owner_reaps_cli_and_its_child() {
    let dir = tempfile::tempdir().unwrap();
    fake_opener(dir.path());
    let marker = dir.path().join("crash");
    let owner = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "owner_process", "--nocapture"])
        .env("HIDED_OWNED_OPENER_TEST_MARKER", &marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut owner = TestOwner(owner);
    let pid = wait_for_pid(&sidecar(&marker, "pid"));
    let child = wait_for_pid(&sidecar(&marker, "child"));
    assert!(alive(pid) && alive(child));
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    assert_gone(pid);
    assert_gone(child);
}

fn seed_registration(dir: &Path, checkout: &Path) {
    std::fs::write(
        dir.join("core-state.json"),
        json!({
            "schema_version": 1,
            "expanded_paths": [],
            "selected_path": Value::Null,
            "selected_pane_id": Value::Null,
            "workspace_registrations": [{
                "id": "w-opener-test",
                "label": "opener-test",
                "path": checkout.display().to_string()
            }]
        })
        .to_string(),
    )
    .unwrap();
}

fn start_private_daemon(dir: &Path, script: &Path) -> (TestOwner, hided::state_file::DaemonState) {
    let child = Command::new(env!("CARGO_BIN_EXE_hided"))
        .env("HOME", dir)
        .env("HIDE_STATE_DIR", dir)
        .env("HERDR_SOCKET_PATH", dir.join("no-herdr.sock"))
        .env("HIDE_OPEN_COMMAND", script)
        .env("HIDE_KEEP_ALIVE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let owner = TestOwner(child);
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(Some(state)) = hided::state_file::read_state(dir) {
            return (owner, state);
        }
        assert!(
            Instant::now() < until,
            "private daemon did not become ready"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(state: &hided::state_file::DaemonState) -> Socket {
    let mut request = format!("ws://127.0.0.1:{}/ws", state.port)
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        ORIGIN,
        format!("http://127.0.0.1:{}", state.port).parse().unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    socket
        .send(Message::Text(
            json!({"token":state.token,"schema_version":2})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let first = next_json(&mut socket).await;
    assert_eq!(first["type"], "snapshot");
    socket
}

async fn next_json(socket: &mut Socket) -> Value {
    let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(frame.to_text().unwrap()).unwrap()
}

async fn open(socket: &mut Socket, file: &Path) -> Value {
    socket
        .send(Message::Text(
            json!({"schema_version":2,"kind":"open_external","payload":{"path":file.display().to_string()}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    loop {
        let frame = next_json(socket).await;
        if frame["type"] == "open_external_result" {
            return frame["payload"].clone();
        }
    }
}

async fn wait_for_openers_to_finish(port: u16) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        let start = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap()
            + 4;
        let body = &response[start..];
        let health: Value = serde_json::from_slice(body).unwrap();
        if health["open_handlers_in_flight"] == 0 {
            return;
        }
        assert!(Instant::now() < until, "quick opener did not finish");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn daemon_caps_in_flight_and_ends_children_after_sigkill() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let checkout = root.join("checkout");
    std::fs::create_dir(&checkout).unwrap();
    seed_registration(&root, &checkout);
    let script = fake_opener(&root);
    let (mut daemon, state) = start_private_daemon(&root, &script);
    let mut socket = connect(&state).await;
    let mut pids = Vec::new();
    for index in 0..4 {
        let file = checkout.join(format!("note-{index}.txt"));
        std::fs::write(&file, "safe data").unwrap();
        assert_eq!(open(&mut socket, &file).await["ok"], true);
        pids.push(wait_for_pid(&sidecar(&file, "pid")));
        pids.push(wait_for_pid(&sidecar(&file, "child")));
    }
    let fifth = checkout.join("note-4.txt");
    std::fs::write(&fifth, "safe data").unwrap();
    let refused = open(&mut socket, &fifth).await;
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["reason"], "over_budget");
    daemon.0.kill().unwrap();
    daemon.0.wait().unwrap();
    for pid in pids {
        assert_gone(pid);
    }
}

#[tokio::test]
async fn daemon_times_out_owned_cli_helper() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let checkout = root.join("checkout");
    std::fs::create_dir(&checkout).unwrap();
    seed_registration(&root, &checkout);
    let script = fake_opener(&root);
    let (_daemon, state) = start_private_daemon(&root, &script);
    let mut socket = connect(&state).await;
    let file = checkout.join("timeout.txt");
    std::fs::write(&file, "safe data").unwrap();
    assert_eq!(open(&mut socket, &file).await["ok"], true);
    let pid = wait_for_pid(&sidecar(&file, "pid"));
    let child = wait_for_pid(&sidecar(&file, "child"));
    assert_gone_after_timeout(pid);
    assert_gone(child);
}

fn assert_gone_after_timeout(pid: i32) {
    let until = Instant::now() + Duration::from_secs(13);
    while alive(pid) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !alive(pid),
        "owned fake opener {pid} survived the 10s timeout"
    );
}

#[tokio::test]
async fn daemon_rejects_thirteenth_open_in_a_minute() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let checkout = root.join("checkout");
    std::fs::create_dir(&checkout).unwrap();
    seed_registration(&root, &checkout);
    let script = root.join("quick-opener");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (_daemon, state) = start_private_daemon(&root, &script);
    let mut socket = connect(&state).await;
    for index in 0..13 {
        let file = checkout.join(format!("quick-{index}.txt"));
        std::fs::write(&file, "safe data").unwrap();
        let answer = open(&mut socket, &file).await;
        if index < 12 {
            assert_eq!(answer["ok"], true, "request {index}");
            wait_for_openers_to_finish(state.port).await;
        } else {
            assert_eq!(answer["reason"], "over_budget");
        }
    }
}
