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
        home: dir.path().to_path_buf(),
        herdr_socket_path: None,
        herdr_bin_path: None,
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

fn handshake_from(token: &str, have_revision: u64, have_terminal_sequence: u64) -> Message {
    Message::Text(
        json!({
            "token": token,
            "schema_version": 2,
            "have_revision": have_revision,
            "have_terminal_sequence": have_terminal_sequence,
        })
        .to_string()
        .into(),
    )
}

async fn first_frame(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    let message = socket.next().await.unwrap().unwrap();
    let Message::Text(text) = message else {
        panic!("expected a text frame");
    };
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn a_reconnect_resumes_from_the_client_cursor() {
    let (_dir, running) = start().await;
    let mut fresh = connect(running.port, None).await;
    fresh.send(handshake(&running.token, 2)).await.unwrap();
    let full = first_frame(&mut fresh).await;
    assert_eq!(full["type"], "snapshot");
    assert!(
        full["payload"]["rest"].is_object(),
        "a snapshot carries rest"
    );
    let revision = full["payload"]["revision"].as_u64().unwrap();
    let sequence = full["payload"]["terminal_sequence"].as_u64().unwrap();

    // Same cursor the first client applied: nothing changed, so a delta
    // without the rest section, not a second full snapshot.
    let mut resumed = connect(running.port, None).await;
    resumed
        .send(handshake_from(&running.token, revision, sequence))
        .await
        .unwrap();
    let delta = first_frame(&mut resumed).await;
    assert_eq!(delta["type"], "delta");
    assert!(
        delta["payload"]["rest"].is_null(),
        "a delta at the current revision has no rest"
    );
    assert_eq!(delta["payload"]["revision"], revision);

    // A cursor ahead of the daemon (it restarted): the client state is not one
    // a delta applies to, so it gets a self-contained snapshot again.
    let mut ahead = connect(running.port, None).await;
    ahead
        .send(handshake_from(&running.token, revision + 1000, sequence))
        .await
        .unwrap();
    let resync = first_frame(&mut ahead).await;
    assert_eq!(resync["type"], "snapshot");
    assert!(resync["payload"]["rest"].is_object());
    assert_eq!(resync["payload"]["revision"], revision);
    running.stop();
}

#[tokio::test]
async fn the_daemon_exits_after_its_last_client_leaves() {
    let (dir, mut env) = test_env(false);
    env.idle_secs = 1;
    let running = std::sync::Arc::new(hided::start_daemon(env).await.expect("start daemon"));
    let state_path = state_file::state_path(dir.path());
    assert!(state_path.exists());
    let mut socket = connect(running.port, None).await;
    socket.send(handshake(&running.token, 2)).await.unwrap();
    let _ = first_frame(&mut socket).await;
    // Held past the idle window: a connected client keeps the daemon up.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        state_path.exists(),
        "a connected client keeps the daemon alive"
    );
    // Registered before the client leaves so the announcement cannot be missed.
    let announced = tokio::spawn({
        let running = std::sync::Arc::clone(&running);
        async move { hided::wait_shutdown(&running).await }
    });
    socket.close(None).await.unwrap();
    drop(socket);
    let stopped = tokio::time::timeout(Duration::from_secs(5), announced).await;
    assert!(
        stopped.is_ok(),
        "the daemon did not announce shutdown after its last client left"
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        let listening = tokio::net::TcpStream::connect(("127.0.0.1", running.port))
            .await
            .is_ok();
        if !listening && !state_path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the listener or the state file outlived the daemon");
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

// --- $HOME boundary over the socket (PRD S2 B10) -------------------------

async fn live_socket(
    running: &hided::RunningDaemon,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let mut socket = connect(running.port, None).await;
    socket.send(handshake(&running.token, 2)).await.unwrap();
    let first = first_frame(&mut socket).await;
    assert_eq!(first["type"], "snapshot");
    socket
}

async fn send_event(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    kind: &str,
    payload: Value,
) -> Value {
    socket
        .send(Message::Text(
            json!({"schema_version": 2, "kind": kind, "payload": payload})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    loop {
        let frame = first_frame(socket).await;
        // Snapshot deltas keep flowing on the same socket; the reply to a
        // boundary event is the first frame that is not one of them.
        if frame["type"] != "snapshot" && frame["type"] != "delta" {
            return frame;
        }
    }
}

#[tokio::test]
async fn local_listing_and_refusals_are_answered_by_hided() {
    let (dir, running) = start().await;
    let home = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(home.join("projects/alpha")).unwrap();
    std::fs::create_dir_all(home.join("projects/.hidden")).unwrap();
    std::fs::write(home.join("projects/file.txt"), "x").unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), home.join("projects/escape")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("nope"), home.join("projects/dangling"))
        .unwrap();
    let mut socket = live_socket(&running).await;

    let listing = send_event(
        &mut socket,
        "remote_file_list",
        json!({"target_id": "local", "root_path": home.join("projects").display().to_string()}),
    )
    .await;
    assert_eq!(listing["type"], "directory_list");
    let names: Vec<&str> = listing["payload"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["alpha"],
        "no file, no hidden dir, no escaping symlink"
    );
    assert_eq!(listing["payload"]["kind"], "remote_file_list");
    assert_eq!(
        listing["payload"]["entries"][0]["is_directory"], true,
        "the registration listing carries directories only"
    );

    let cases = [
        (
            home.join("projects/escape").display().to_string(),
            "outside_home",
        ),
        (
            home.join("projects/escape/nope").display().to_string(),
            "outside_home",
        ),
        (
            home.join("projects/dangling").display().to_string(),
            "outside_home",
        ),
        (
            home.join("projects/dangling/child").display().to_string(),
            "outside_home",
        ),
        (format!("{}/projects/../..", home.display()), "invalid_path"),
        (
            format!("{}/projects/%2e%2e/%2e%2e", home.display()),
            "not_found",
        ),
        (
            home.join("projects/file.txt").display().to_string(),
            "not_a_directory",
        ),
        (outside.path().display().to_string(), "outside_home"),
        (
            outside.path().join("nope").display().to_string(),
            "outside_home",
        ),
        (home.display().to_string(), "home_root"),
        ("relative/path".to_owned(), "invalid_path"),
    ];
    for (path, reason) in cases {
        let refused = send_event(
            &mut socket,
            "create_workspace",
            json!({"path": path, "label": "x", "initialize_git": false}),
        )
        .await;
        assert_eq!(refused["type"], "path_refused", "{path}");
        assert_eq!(refused["payload"]["reason"], reason, "{path}");
        assert_eq!(refused["payload"]["kind"], "create_workspace");
    }
    let refused_list = send_event(
        &mut socket,
        "remote_file_list",
        json!({"target_id": "local", "root_path": outside.path().display().to_string()}),
    )
    .await;
    assert_eq!(refused_list["payload"]["reason"], "outside_home");
    assert_eq!(refused_list["payload"]["kind"], "remote_file_list");
    running.stop();
}

#[tokio::test]
async fn a_remote_listing_for_another_target_is_forwarded_untouched() {
    let (_dir, running) = start().await;
    let outside = tempfile::tempdir().unwrap();
    let mut socket = live_socket(&running).await;
    // The path is on the remote machine; locally it is outside home, so a
    // boundary that wrongly ran would answer `path_refused`. The core has no
    // such target and records that instead, which shows the event arrived.
    socket
        .send(Message::Text(
            json!({
                "schema_version": 2,
                "kind": "remote_file_list",
                "payload": {"target_id": "mini", "root_path": outside.path().display().to_string()},
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let reaction = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frame = first_frame(&mut socket).await;
            assert_ne!(
                frame["type"], "path_refused",
                "a non-local listing must not meet the local boundary"
            );
            if frame.to_string().contains("remote.files.unknown_target") {
                return frame;
            }
        }
    })
    .await
    .expect("the core's reaction to the forwarded event");
    assert!(matches!(
        reaction["type"].as_str(),
        Some("delta" | "snapshot")
    ));
    running.stop();
}

// --- checkout-root boundary over the socket (PRD S3 B7) ------------------

/// A client socket, as `connect` builds it.
type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Sends an event and returns the first frame that answers it: a refusal from
/// hided, or the core's own reaction to a forwarded event. `answered` names
/// the reaction, so a delta the core emitted for something else does not count
/// as the answer.
async fn send_event_expecting(
    socket: &mut Socket,
    kind: &str,
    payload: Value,
    answered: impl Fn(&Value) -> bool,
) -> Value {
    socket
        .send(Message::Text(
            json!({"schema_version": 2, "kind": kind, "payload": payload})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frame = first_frame(socket).await;
            if frame["type"] == "path_refused" || answered(&frame) {
                return frame;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("an answer to the {kind} event"))
}

/// The reaction predicate of a step whose only expected answer is the refusal
/// itself, which the loop returns before it asks.
fn never(_: &Value) -> bool {
    false
}

/// Writes the core's own state file with one registration, which is what makes
/// a directory a checkout and so what gives hided a root to check against. The
/// file is the shape herdr-core persists: flat, schema 1, no nesting.
fn seed_registration(state_dir: &std::path::Path, id: &str, path: &std::path::Path) {
    std::fs::write(
        state_dir.join("core-state.json"),
        json!({
            "schema_version": 1,
            "expanded_paths": [],
            "selected_path": Value::Null,
            "selected_pane_id": Value::Null,
            "workspace_registrations": [{
                "id": id,
                "label": "alpha",
                "path": path.display().to_string(),
            }],
        })
        .to_string(),
    )
    .unwrap();
}

#[tokio::test]
async fn explorer_paths_are_checked_against_the_registered_checkout() {
    let (dir, env) = test_env(true);
    let home = dir.path().canonicalize().unwrap();
    let checkout = home.join("projects/alpha");
    std::fs::create_dir_all(checkout.join("src")).unwrap();
    std::fs::write(checkout.join("src/main.rs"), "fn main() {}\n").unwrap();
    // A directory under home that no registration covers: the registration
    // line reads it, the Explorer line must not.
    let notes = home.join("projects/notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::create_dir_all(notes.join("plans")).unwrap();
    std::fs::write(notes.join("todo.md"), "- [ ] x\n").unwrap();
    seed_registration(&env.state_dir, "w-alpha", &checkout);
    let running = hided::start_daemon(env).await.expect("start daemon");

    let mut socket = connect(running.port, None).await;
    socket.send(handshake(&running.token, 2)).await.unwrap();
    let snapshot = first_frame(&mut socket).await;
    assert_eq!(snapshot["type"], "snapshot");
    let workspace = snapshot["payload"]["rest"]["navigator"]["workspaces"]
        .as_array()
        .expect("the snapshot carries the navigator")
        .iter()
        .find(|workspace| workspace["path"] == json!(checkout.display().to_string()))
        .cloned()
        .expect("the seeded registration is a workspace");
    let workspace_id = workspace["id"].as_str().unwrap().to_owned();
    let checkout_id = workspace["checkouts"][0]["id"].as_str().unwrap().to_owned();
    let checkout_path = checkout.display().to_string();
    let notes_path = notes.join("todo.md").display().to_string();
    let file_path = checkout.join("src/main.rs").display().to_string();

    // A path under home that no checkout covers is refused as such, for an
    // open and for a change alike.
    let refused = send_event_expecting(
        &mut socket,
        "file_open",
        json!({
            "path": notes_path,
            "workspace_id": workspace_id,
            "checkout_id": checkout_id,
            "preview": false,
        }),
        never,
    )
    .await;
    assert_eq!(refused["type"], "path_refused");
    assert_eq!(refused["payload"]["kind"], "file_open");
    assert_eq!(refused["payload"]["reason"], "outside_checkout");
    assert_eq!(refused["payload"]["path"], notes_path);

    let refused = send_event_expecting(
        &mut socket,
        "path_trash",
        json!({
            "root": checkout_path,
            "path": notes_path,
            "select_after": checkout_path,
            "inode": Value::Null,
        }),
        never,
    )
    .await;
    assert_eq!(refused["payload"]["kind"], "path_trash");
    assert_eq!(refused["payload"]["reason"], "outside_checkout");

    // The root a change names has to be one of the registered ones.
    let unregistered_root = home.join("projects").display().to_string();
    let refused = send_event_expecting(
        &mut socket,
        "path_trash",
        json!({
            "root": unregistered_root,
            "path": checkout.join("src").display().to_string(),
            "select_after": checkout_path,
            "inode": Value::Null,
        }),
        never,
    )
    .await;
    assert_eq!(refused["payload"]["kind"], "path_trash");
    assert_eq!(refused["payload"]["reason"], "outside_checkout");
    assert_eq!(refused["payload"]["path"], unregistered_root);

    // A name that is not one component never reaches the core.
    let refused = send_event_expecting(
        &mut socket,
        "path_rename",
        json!({"root": checkout_path, "path": file_path, "name": "../escape.rs"}),
        never,
    )
    .await;
    assert_eq!(refused["payload"]["kind"], "path_rename");
    assert_eq!(refused["payload"]["reason"], "invalid_path");
    assert_eq!(refused["payload"]["path"], "../escape.rs");

    // A path inside the registered checkout is the Explorer's: the core opens
    // it, and the tab it makes carries the file's own name.
    let opened = send_event_expecting(
        &mut socket,
        "file_open",
        json!({
            "path": file_path,
            "workspace_id": workspace_id,
            "checkout_id": checkout_id,
            "preview": false,
        }),
        |frame| frame["payload"]["editor"]["tabs"][0]["label"] == "main.rs",
    )
    .await;
    assert_eq!(opened["type"], "delta", "the open has to reach the core");
    assert_eq!(
        opened["payload"]["editor"]["tabs"][0]["label"], "main.rs",
        "the tab the core makes carries the file's own name"
    );

    // The two lines stay separate. The registration line answers for the home
    // tree on its own, so a directory under home that no checkout covers is
    // listed rather than refused: the checkout line would have called this
    // path `outside_checkout`.
    let listing = send_event_expecting(
        &mut socket,
        "remote_file_list",
        json!({"target_id": "local", "root_path": notes.display().to_string()}),
        |frame| frame["type"] == "directory_list",
    )
    .await;
    assert_eq!(listing["type"], "directory_list");
    let names: Vec<&str> = listing["payload"]["entries"]
        .as_array()
        .expect("a listing carries entries")
        .iter()
        .map(|entry| entry["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["plans"], "no file, only the directory");
    running.stop();
}

#[tokio::test]
async fn the_explorer_listing_shows_a_checkout_folder_in_the_swift_order() {
    let (dir, env) = test_env(true);
    let home = dir.path().canonicalize().unwrap();
    let checkout = home.join("projects/alpha");
    std::fs::create_dir_all(checkout.join("src")).unwrap();
    std::fs::create_dir_all(checkout.join(".github")).unwrap();
    std::fs::create_dir_all(checkout.join(".git/objects")).unwrap();
    std::fs::write(checkout.join("README.md"), "# alpha\n").unwrap();
    std::fs::write(checkout.join("file2.txt"), "x").unwrap();
    std::fs::write(checkout.join("file10.txt"), "x").unwrap();
    std::fs::write(checkout.join(".env"), "x").unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), checkout.join("escape")).unwrap();
    std::os::unix::fs::symlink(checkout.join("src"), checkout.join("alias")).unwrap();
    // A folder under home that no registration covers: the registration line
    // reads it, the Explorer line must not.
    let notes = home.join("projects/notes");
    std::fs::create_dir_all(&notes).unwrap();
    seed_registration(&env.state_dir, "w-alpha", &checkout);
    let running = hided::start_daemon(env).await.expect("start daemon");
    let mut socket = live_socket(&running).await;
    let checkout_path = checkout.display().to_string();
    let notes_path = notes.display().to_string();

    let listing = send_event_expecting(
        &mut socket,
        "file_list",
        json!({"root": checkout_path, "path": checkout_path}),
        |frame| frame["type"] == "directory_list",
    )
    .await;
    assert_eq!(listing["type"], "directory_list");
    assert_eq!(listing["payload"]["kind"], "file_list");
    assert_eq!(listing["payload"]["root_path"], json!(checkout_path));
    assert_eq!(listing["payload"]["truncated"], false);
    let rows: Vec<(&str, bool)> = listing["payload"]["entries"]
        .as_array()
        .expect("a listing carries entries")
        .iter()
        .map(|entry| {
            (
                entry["name"].as_str().unwrap(),
                entry["is_directory"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (".github", true),
            ("alias", true),
            ("src", true),
            (".env", false),
            ("file2.txt", false),
            ("file10.txt", false),
            ("README.md", false),
        ],
        "directories first and then the natural order; .git and the symlink that \
         leaves the root are not rows"
    );

    // A folder inside the root is the Explorer's at any depth.
    let nested = send_event_expecting(
        &mut socket,
        "file_list",
        json!({"root": checkout_path, "path": checkout.join("src").display().to_string()}),
        |frame| frame["type"] == "directory_list",
    )
    .await;
    assert_eq!(
        nested["payload"]["root_path"],
        json!(checkout.join("src").display().to_string())
    );
    assert!(nested["payload"]["entries"].as_array().unwrap().is_empty());

    // The root the listing names has to be a registered one, and the folder has
    // to be under it: a folder under home that no checkout covers, and a
    // symlink out of the checkout, are both refused as outside_checkout.
    for (root, path) in [
        (checkout_path.clone(), notes_path.clone()),
        (notes_path.clone(), notes_path.clone()),
        (
            checkout_path.clone(),
            checkout.join("escape").display().to_string(),
        ),
    ] {
        let refused = send_event_expecting(
            &mut socket,
            "file_list",
            json!({"root": root, "path": path}),
            never,
        )
        .await;
        assert_eq!(refused["type"], "path_refused", "{root} {path}");
        assert_eq!(refused["payload"]["kind"], "file_list");
        assert_eq!(refused["payload"]["reason"], "outside_checkout");
    }
    running.stop();
}

/// Reads the next binary frame and splits it into its header and bytes, the
/// way a viewer does: a 4-byte big-endian header length, the header JSON, then
/// the payload. Text frames (snapshot deltas) are skipped.
async fn recv_binary(socket: &mut Socket) -> (Value, Vec<u8>) {
    loop {
        let message = socket.next().await.unwrap().unwrap();
        let Message::Binary(bytes) = message else {
            continue;
        };
        let header_len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let header: Value = serde_json::from_slice(&bytes[4..4 + header_len]).unwrap();
        return (header, bytes[4 + header_len..].to_vec());
    }
}

#[tokio::test]
async fn file_bytes_streams_a_checkout_file_and_refuses_a_path_outside_it() {
    let (dir, env) = test_env(true);
    let home = dir.path().canonicalize().unwrap();
    let checkout = home.join("projects/alpha");
    std::fs::create_dir_all(&checkout).unwrap();
    let contents: Vec<u8> = (0u8..64).collect();
    std::fs::write(checkout.join("blob.bin"), &contents).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.bin"), b"x").unwrap();
    seed_registration(&env.state_dir, "w-alpha", &checkout);
    let running = hided::start_daemon(env).await.expect("start daemon");
    let mut socket = live_socket(&running).await;
    let path = checkout.join("blob.bin").display().to_string();

    // A whole-file read: one binary frame carrying the header and every byte.
    socket
        .send(Message::Text(
            json!({
                "schema_version": 2,
                "kind": "file_bytes",
                "payload": {"request_id": "r1", "path": path, "offset": 0, "length": Value::Null},
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let (header, payload) = recv_binary(&mut socket).await;
    assert_eq!(header["type"], "file_bytes");
    assert_eq!(header["request_id"], "r1");
    assert_eq!(header["path"], json!(path));
    assert_eq!(header["offset"], 0);
    assert_eq!(header["total"], 64);
    assert_eq!(header["eof"], true);
    assert_eq!(payload, contents);

    // A range read names its own offset and returns only those bytes; the
    // total still reports the whole file, which is what a seek needs.
    socket
        .send(Message::Text(
            json!({
                "schema_version": 2,
                "kind": "file_bytes",
                "payload": {"request_id": "r2", "path": path, "offset": 16, "length": 16},
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let (header, payload) = recv_binary(&mut socket).await;
    assert_eq!(header["request_id"], "r2");
    assert_eq!(header["offset"], 16);
    assert_eq!(header["total"], 64);
    assert_eq!(header["eof"], true);
    assert_eq!(payload, contents[16..32]);

    // A file outside every registered root is the same path_refused frame the
    // rest of the boundary answers with.
    let refused = send_event_expecting(
        &mut socket,
        "file_bytes",
        json!({
            "request_id": "r3",
            "path": outside.path().join("secret.bin").display().to_string(),
            "offset": 0,
            "length": Value::Null,
        }),
        never,
    )
    .await;
    assert_eq!(refused["type"], "path_refused");
    assert_eq!(refused["payload"]["kind"], "file_bytes");
    assert_eq!(refused["payload"]["reason"], "outside_checkout");

    // A directory under the root is not a byte source.
    let refused = send_event_expecting(
        &mut socket,
        "file_bytes",
        json!({
            "request_id": "r4",
            "path": checkout.display().to_string(),
            "offset": 0,
            "length": Value::Null,
        }),
        never,
    )
    .await;
    assert_eq!(refused["type"], "path_refused");
    assert_eq!(refused["payload"]["reason"], "not_a_file");

    // A read past the cap is answered as a one-line failure, not streamed.
    let refused = send_event_expecting(
        &mut socket,
        "file_bytes",
        json!({
            "request_id": "r5",
            "path": path,
            "offset": 0,
            "length": hided::boundary::MAX_FILE_BYTES + 1,
        }),
        |frame| frame["type"] == "file_bytes_error",
    )
    .await;
    assert_eq!(refused["type"], "file_bytes_error");
    assert_eq!(refused["payload"]["reason"], "too_large");
    running.stop();
}

#[tokio::test]
async fn a_change_in_a_watched_checkout_is_announced() {
    let (dir, env) = test_env(true);
    let home = dir.path().canonicalize().unwrap();
    let checkout = home.join("projects/alpha");
    std::fs::create_dir_all(checkout.join("src")).unwrap();
    seed_registration(&env.state_dir, "w-alpha", &checkout);
    let running = hided::start_daemon(env).await.expect("start daemon");
    let mut socket = live_socket(&running).await;
    // The watcher arms after the daemon's first snapshot read; let it settle
    // so the change below is not missed by a watcher that was not up yet.
    tokio::time::sleep(Duration::from_millis(750)).await;
    std::fs::write(checkout.join("appeared.txt"), "x").unwrap();
    let frame = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frame = first_frame(&mut socket).await;
            if frame["type"] == "directory_changed" {
                return frame;
            }
        }
    })
    .await
    .expect("a directory_changed frame for the changed checkout");
    assert_eq!(
        frame["payload"]["path"],
        json!(checkout.display().to_string())
    );
    running.stop();
}

#[tokio::test]
async fn the_file_index_answers_the_ranked_matches_for_a_checkout() {
    let (dir, env) = test_env(true);
    let home = dir.path().canonicalize().unwrap();
    let checkout = home.join("projects/alpha");
    std::fs::create_dir_all(checkout.join("src")).unwrap();
    std::fs::create_dir_all(checkout.join("docs")).unwrap();
    std::fs::write(checkout.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(checkout.join("docs/readme.md"), "# a\n").unwrap();
    std::fs::write(checkout.join("debug.log"), "x").unwrap();
    std::fs::write(checkout.join(".gitignore"), "*.log\n").unwrap();
    seed_registration(&env.state_dir, "w-alpha", &checkout);
    let running = hided::start_daemon(env).await.expect("start daemon");
    let mut socket = live_socket(&running).await;
    let root = checkout.display().to_string();

    // The first query starts the walk; the daemon answers `indexing`, and the
    // next query has the list.
    let mut result = send_event_expecting(
        &mut socket,
        "file_index",
        json!({"root": root, "query": "main"}),
        |frame| frame["type"] == "file_index_result",
    )
    .await;
    for _ in 0..40 {
        if result["payload"]["indexing"] == json!(false) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        result = send_event_expecting(
            &mut socket,
            "file_index",
            json!({"root": root, "query": "main"}),
            |frame| frame["type"] == "file_index_result",
        )
        .await;
    }
    assert_eq!(result["payload"]["indexing"], json!(false));
    assert_eq!(result["payload"]["truncated"], json!(false));
    let entries: Vec<&str> = result["payload"]["files"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| entry["relative_path"].as_str().unwrap())
        .collect();
    assert_eq!(
        entries,
        vec!["src/main.rs"],
        "only the match, and not the ignored file"
    );
    assert_eq!(
        result["payload"]["files"][0]["path"],
        json!(checkout.join("src/main.rs").display().to_string()),
        "a palette entry carries the absolute path to open"
    );

    // A root that is not a registered checkout is refused.
    let refused = send_event_expecting(
        &mut socket,
        "file_index",
        json!({"root": home.join("projects").display().to_string(), "query": "x"}),
        never,
    )
    .await;
    assert_eq!(refused["type"], "path_refused");
    assert_eq!(refused["payload"]["kind"], "file_index");
    assert_eq!(refused["payload"]["reason"], "outside_checkout");
    running.stop();
}

#[tokio::test]
async fn open_external_checks_the_checkout_boundary_first() {
    let (dir, env) = test_env(true);
    let home = dir.path().canonicalize().unwrap();
    let checkout = home.join("projects/alpha");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::write(checkout.join("notes.txt"), "x").unwrap();
    seed_registration(&env.state_dir, "w-alpha", &checkout);
    let running = hided::start_daemon(env).await.expect("start daemon");
    let mut socket = live_socket(&running).await;

    // A path outside every registered root never reaches the host handler.
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "x").unwrap();
    let refused = send_event_expecting(
        &mut socket,
        "open_external",
        json!({"path": outside.path().join("secret.txt").display().to_string()}),
        never,
    )
    .await;
    assert_eq!(refused["type"], "path_refused");
    assert_eq!(refused["payload"]["kind"], "open_external");
    assert_eq!(refused["payload"]["reason"], "outside_checkout");

    // A directory is not a file to hand to the handler.
    let refused = send_event_expecting(
        &mut socket,
        "open_external",
        json!({"path": checkout.display().to_string()}),
        never,
    )
    .await;
    assert_eq!(refused["payload"]["reason"], "not_a_file");

    // A program, a terminal script, an application bundle or an installer is
    // inside the boundary and still never reaches the host handler: the answer
    // is an ok:false result rather than a path refusal (D-12). The `.terminal`
    // file is an ordinary 0644 plist that Terminal would run on open.
    for name in ["run.sh", "thing.dmg", "note.terminal", "job.command"] {
        let target = checkout.join(name);
        std::fs::write(&target, "x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if name.ends_with(".sh") {
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let answer = send_event_expecting(
            &mut socket,
            "open_external",
            json!({"path": target.display().to_string()}),
            |frame| frame["type"] == "open_external_result",
        )
        .await;
        assert_eq!(answer["payload"]["ok"], false, "{name}");
        assert_eq!(answer["payload"]["reason"], "not_openable", "{name}");
    }
    running.stop();
}

#[tokio::test]
async fn a_client_cannot_send_the_shell_attachment_events() {
    let (_dir, running) = start().await;
    let mut socket = live_socket(&running).await;
    // The browser stages bytes through attachment_*; a client that sends the
    // shell's own event would name an arbitrary path for the core to read.
    for kind in [
        "terminal_attachment",
        "terminal_attachment_ready",
        "terminal_attachment_action",
    ] {
        let refused = send_event_expecting(
            &mut socket,
            kind,
            json!({
                "request_id": "01234567-0123-0123-0123-0123456789ab",
                "pane_id": "p1",
                "paths": ["/etc/hosts"],
                "error": Value::Null,
                "action": "cancel",
            }),
            |frame| frame["type"] == "error",
        )
        .await;
        assert_eq!(refused["type"], "error", "{kind}");
    }
    running.stop();
}

#[tokio::test]
async fn attachment_stages_over_the_cap_and_unknown_commits_are_refused() {
    let (_dir, running) = start().await;
    let mut socket = live_socket(&running).await;

    let refused = send_event_expecting(
        &mut socket,
        "attachment_stage",
        json!({
            "request_id": "r1",
            "name": "big.bin",
            "size": (20u64 * 1024 * 1024) + 1,
            "clipboard": false,
        }),
        |frame| frame["type"] == "attachment_refused",
    )
    .await;
    assert_eq!(refused["type"], "attachment_refused");
    assert_eq!(refused["payload"]["reason"], "too_large");

    // A stage id that names a path is refused before anything is written.
    let refused = send_event_expecting(
        &mut socket,
        "attachment_stage",
        json!({
            "request_id": "../escape",
            "name": "shot.png",
            "size": 1,
            "clipboard": false,
        }),
        |frame| frame["type"] == "attachment_refused",
    )
    .await;
    assert_eq!(refused["payload"]["reason"], "invalid_request_id");

    let refused = send_event_expecting(
        &mut socket,
        "attachment_commit",
        json!({
            "request_id": "01234567-0123-0123-0123-0123456789ab",
            "pane_id": "p1",
            "bracketed_paste": true,
            "clipboard": false,
            "stages": ["never-staged"],
        }),
        |frame| frame["type"] == "attachment_refused",
    )
    .await;
    assert_eq!(refused["payload"]["reason"], "unknown_stage");

    // A commit id the core would refuse is refused before any stage is consumed.
    let refused = send_event_expecting(
        &mut socket,
        "attachment_commit",
        json!({
            "request_id": "not-a-uuid",
            "pane_id": "p1",
            "bracketed_paste": true,
            "clipboard": false,
            "stages": ["never-staged"],
        }),
        |frame| frame["type"] == "attachment_refused",
    )
    .await;
    assert_eq!(refused["payload"]["reason"], "invalid_request_id");

    // A clipboard commit must name its own stage; a different id is refused
    // (the mode binding itself is covered by the attachments unit tests).
    let refused = send_event_expecting(
        &mut socket,
        "attachment_commit",
        json!({
            "request_id": "b2",
            "pane_id": "p1",
            "bracketed_paste": true,
            "clipboard": true,
            "stages": ["never-staged"],
        }),
        |frame| frame["type"] == "attachment_refused",
    )
    .await;
    assert_eq!(refused["payload"]["reason"], "invalid_request_id");

    // A clipboard commit must name the one stage that wrote its image, because
    // the core reads the file its own request id derives.
    let refused = send_event_expecting(
        &mut socket,
        "attachment_commit",
        json!({
            "request_id": "01234567-0123-0123-0123-0123456789ab",
            "pane_id": "p1",
            "bracketed_paste": true,
            "clipboard": true,
            "stages": [],
        }),
        |frame| frame["type"] == "attachment_refused",
    )
    .await;
    assert_eq!(refused["payload"]["reason"], "invalid_request_id");
    running.stop();
}
