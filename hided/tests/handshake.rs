use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use hided::env::Env;
use hided::state_file;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::ORIGIN;

fn test_env(keep_alive: bool) -> (tempfile::TempDir, Env) {
    let dir = tempfile::tempdir().unwrap();
    let env = Env {
        herdr_socket_path: None,
        state_dir: dir.path().to_path_buf(),
        keep_alive,
        vite_origin: None,
        bind: "127.0.0.1:0".parse().unwrap(),
        idle_secs: 600,
    };
    (dir, env)
}

async fn start() -> (tempfile::TempDir, hided::RunningDaemon) {
    let (dir, env) = test_env(true);
    let running = hided::start_daemon(env).await.expect("start daemon");
    (dir, running)
}

async fn connect(
    port: u16,
    origin: Option<&str>,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let mut request = format!("ws://127.0.0.1:{port}/ws")
        .into_client_request()
        .unwrap();
    let origin = origin
        .map(str::to_owned)
        .unwrap_or_else(|| format!("http://127.0.0.1:{port}"));
    request
        .headers_mut()
        .insert(ORIGIN, origin.parse().unwrap());
    let (socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("ws connect");
    socket
}

fn handshake(token: &str, schema: u32) -> Message {
    Message::Text(
        json!({"token": token, "schema_version": schema})
            .to_string()
            .into(),
    )
}

#[tokio::test]
async fn health_and_unknown_http_do_not_dispatch() {
    let (_dir, running) = start().await;
    let url = format!("http://127.0.0.1:{}/health", running.port);
    let body = reqwest_get(&url).await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["schema_version"], 2);
    let post = reqwest_post(&format!("http://127.0.0.1:{}/dispatch", running.port)).await;
    assert_eq!(post, 404, "unknown HTTP paths must be 404");
    let missing = reqwest_status(&format!("http://127.0.0.1:{}/nope", running.port)).await;
    assert_eq!(missing, 404);
    let health_after = reqwest_get(&url).await;
    let after: Value = serde_json::from_str(&health_after).unwrap();
    assert_eq!(after["clients"], 0);
    running.stop();
}

#[tokio::test]
async fn state_file_mode_is_600() {
    let (dir, running) = start().await;
    let path = state_file::state_path(dir.path());
    let mode = std::fs::metadata(path).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(mode.mode() & 0o777, 0o600);
    running.stop();
}

#[tokio::test]
async fn invalid_token_is_refused() {
    let (_dir, running) = start().await;
    let mut socket = connect(running.port, None).await;
    socket.send(handshake("nope", 2)).await.unwrap();
    let close = wait_close(&mut socket).await;
    assert_eq!(close, Some(4001));
    running.stop();
}

#[tokio::test]
async fn schema_mismatch_is_refused() {
    let (_dir, running) = start().await;
    let mut socket = connect(running.port, None).await;
    socket.send(handshake(&running.token, 1)).await.unwrap();
    let close = wait_close(&mut socket).await;
    assert_eq!(close, Some(4003));
    running.stop();
}

#[tokio::test]
async fn origin_not_allowed_is_refused() {
    let (_dir, running) = start().await;
    let mut socket = connect(running.port, Some("http://example.com")).await;
    let close = wait_close(&mut socket).await;
    assert_eq!(close, Some(4002));
    running.stop();
}

#[tokio::test]
async fn traversal_of_the_ui_dir_is_404() {
    let (dir, running) = start().await;
    let code = reqwest_status(&format!(
        "http://127.0.0.1:{}/assets/../../hided.json",
        running.port
    ))
    .await;
    assert_eq!(code, 404);
    let escaped = tokio::process::Command::new("/usr/bin/curl")
        .args([
            "-s",
            "--path-as-is",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            &format!(
                "http://127.0.0.1:{}/assets/../../{}/hided.json",
                running.port,
                dir.path().join("hided.json").display()
            ),
        ])
        .output()
        .await
        .unwrap();
    let body_code = String::from_utf8(escaped.stdout).unwrap();
    assert_eq!(body_code, "404");
    running.stop();
}

#[tokio::test]
async fn ninth_client_is_refused() {
    let (_dir, running) = start().await;
    let mut held = Vec::new();
    for _ in 0..8 {
        let mut socket = connect(running.port, None).await;
        socket.send(handshake(&running.token, 2)).await.unwrap();
        let _ = socket.next().await;
        held.push(socket);
    }
    let mut extra = connect(running.port, None).await;
    extra.send(handshake(&running.token, 2)).await.unwrap();
    let close = wait_close(&mut extra).await;
    assert_eq!(close, Some(4004));
    running.stop();
}

#[tokio::test]
async fn valid_handshake_receives_snapshot() {
    let (_dir, running) = start().await;
    let mut socket = connect(running.port, None).await;
    socket.send(handshake(&running.token, 2)).await.unwrap();
    let message = socket.next().await.unwrap().unwrap();
    let Message::Text(text) = message else {
        panic!("expected text snapshot");
    };
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["type"], "snapshot");
    assert_eq!(value["payload"]["schema_version"], 2);
    running.stop();
}

async fn wait_close(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Option<u16> {
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(frame) = socket.next().await {
            if let Ok(Message::Close(Some(close))) = frame {
                return Some(close.code.into());
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
}

async fn reqwest_get(url: &str) -> String {
    let output = tokio::process::Command::new("/usr/bin/curl")
        .args(["-fsS", url])
        .output()
        .await
        .unwrap();
    String::from_utf8(output.stdout).unwrap()
}

async fn reqwest_post(url: &str) -> u16 {
    let output = tokio::process::Command::new("/usr/bin/curl")
        .args([
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "-X",
            "POST",
            url,
        ])
        .output()
        .await
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .parse()
        .unwrap_or(0)
}

async fn reqwest_status(url: &str) -> u16 {
    let output = tokio::process::Command::new("/usr/bin/curl")
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", url])
        .output()
        .await
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .parse()
        .unwrap_or(0)
}
