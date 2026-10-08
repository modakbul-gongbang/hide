//! The operator notice for a letter that missed its delivery deadline
//! (issue 755), against a real daemon with a seeded ledger. Mobile is in its
//! default state (push mode off), so the phone is skipped before any send;
//! what is left is Herdr, which is either absent or the pinned private
//! server, and the daemon's diagnostic log names both reasons. The ledger is
//! the test's own, so no notice reaches the operator.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use hided::env::Env;
use serde_json::{Value, json};

#[path = "support/private_herdr.rs"]
mod private_herdr;
use private_herdr::PrivateHerdr;

/// The diagnostic sink is one slot per process, so two daemons alive at once
/// would write into each other's log; each test holds this for its whole run.
static ONE_DAEMON: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const BODY: &str = "report the operator never saw";

fn env(root: &Path, herdr: Option<&PrivateHerdr>) -> Env {
    Env {
        home: herdr.map_or_else(|| root.join("home"), |herdr| herdr.home.clone()),
        herdr_socket_path: herdr.map(|herdr| herdr.socket.display().to_string()),
        herdr_bin_path: herdr.map(|herdr| herdr.bin.clone()),
        state_dir: root.join("state"),
        legacy_state_dir: None,
        keep_alive: true,
        vite_origin: None,
        bind: "127.0.0.1:0".parse().unwrap(),
        idle_secs: 600,
        build: None,
        open_command: None,
        host_helper_root: None,
        host_cli_dir: None,
        pane_id: None,
        // A missing path: this test reaches no Tailscale.
        tailscale_bin: Some(root.join("no-tailscale")),
        search_path: None,
    }
}

/// A ledger holding one letter that missed its delivery deadline an hour ago
/// and has not been announced to the operator. Its times are recent because
/// the ledger drops a finished letter once its retention passed.
fn seed_undelivered_letter(state: &Path) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let hour = 60 * 60 * 1000;
    let actor = |name: &str| {
        json!({"pane_id": name, "name": name, "kind": "claude", "device_id": "local",
            "session": format!("{name}-session")})
    };
    let ledger = json!({
        "version": 1, "next_id": 8,
        "letters": [{
            "id": "letter-7", "intent": "held", "sender": actor("sender"),
            "recipient": actor("lead"), "kind": "report", "body": BODY,
            "state": "undelivered", "waiting_answer": false, "reply_to": null,
            "created_at_unix_ms": now - hour, "finished_at_unix_ms": now,
            "bell_errors": 0, "bell_sent": false, "human_notified": false,
        }],
        "watches": [],
    });
    // The seed is a valid ledger as the daemon's loader judges it, so a
    // refusal below names the seed and not the daemon.
    serde_json::from_value::<herdr_core::delivery::ledger::Ledger>(ledger.clone())
        .expect("the seed parses as a ledger")
        .validate()
        .expect("the seed is a valid ledger");
    std::fs::create_dir_all(state).unwrap();
    // The loader refuses a state folder or ledger the account's own mode does
    // not guard.
    std::fs::set_permissions(state, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = state.join("delivery-ledger.json");
    std::fs::write(&path, ledger.to_string()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

/// Writes a store file the daemon will read, readable by this account only.
fn write_private(path: &Path, value: &Value) {
    std::fs::write(path, value.to_string()).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

/// The one `human.channels_failed` row for the seeded letter, once the
/// daemon's first delivery pass wrote it. The deadline only ends a daemon
/// that never reports, and then the whole log is the report.
async fn channels_failed(state: &Path) -> Value {
    let log = state.join("Logs/core.jsonl");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let rows = text
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|row| row["kind"] == "human.channels_failed")
            .collect::<Vec<_>>();
        if let Some(row) = rows.first() {
            assert_eq!(rows.len(), 1, "one notice, one record: {text}");
            assert!(
                !text.contains(BODY),
                "the log never carries a letter's text"
            );
            return row.clone();
        }
        assert!(
            Instant::now() < deadline,
            "no channels_failed record: {text}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn without_a_herdr_socket_the_record_names_both_skipped_channels() {
    let _alone = ONE_DAEMON.lock().await;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("home")).unwrap();
    seed_undelivered_letter(&root.path().join("state"));
    let running = hided::start_daemon(env(root.path(), None))
        .await
        .expect("hided starts");
    let row = channels_failed(&root.path().join("state")).await;
    assert_eq!(row["component"], "delivery");
    assert_eq!(row["notice"], "letter_undelivered");
    assert_eq!(row["letter_id"], "letter-7");
    assert_eq!(row["push"], "mode_off");
    assert_eq!(row["herdr"], "no_socket");
    running.stop();
}

#[tokio::test]
#[ignore = "needs the pinned Herdr: set HIDE_E2E_HERDR_BIN and run with --ignored"]
async fn the_pinned_herdr_is_tried_and_its_reason_is_recorded() {
    let _alone = ONE_DAEMON.lock().await;
    let bin = std::path::PathBuf::from(
        std::env::var_os("HIDE_E2E_HERDR_BIN").expect("HIDE_E2E_HERDR_BIN names the pinned herdr"),
    );
    // A Unix socket path is limited to about a hundred bytes.
    let root = tempfile::Builder::new()
        .prefix("hn")
        .tempdir_in("/tmp")
        .unwrap();
    let mut herdr = PrivateHerdr::start(bin, root.path());
    seed_undelivered_letter(&root.path().join("state"));
    let running = hided::start_daemon(env(root.path(), Some(&herdr)))
        .await
        .expect("hided starts");
    let row = channels_failed(&root.path().join("state")).await;
    assert_eq!(row["notice"], "letter_undelivered");
    assert_eq!(row["letter_id"], "letter-7");
    assert_eq!(row["push"], "mode_off");
    // Herdr's toast is off by default and no client is attached to this
    // server: either way it answered, and the answer is what is recorded.
    let herdr_reason = row["herdr"].as_str().unwrap();
    assert!(
        ["disabled", "no_foreground_client"].contains(&herdr_reason),
        "Herdr's own reason, not a skip: {row}"
    );
    running.stop();
    herdr.stop().expect("the private Herdr server exits");
}

/// A stand-in push service on loopback (a debug build accepts that endpoint):
/// it answers 201 to the first request and reports the request line.
fn fake_push_service() -> (String, std::sync::mpsc::Receiver<String>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!(
        "http://127.0.0.1:{}/push/1",
        listener.local_addr().unwrap().port()
    );
    let (sent, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let _ = sent.send(request_line);
        let _ = reader
            .get_mut()
            .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    });
    (endpoint, received)
}

#[tokio::test]
async fn a_reachable_phone_still_gets_the_notice_and_nothing_is_recorded_as_failed() {
    let _alone = ONE_DAEMON.lock().await;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};

    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("home")).unwrap();
    let state = root.path().join("state");
    seed_undelivered_letter(&state);
    // Push mode always, one paired phone with a subscription: the phone's
    // key is a fresh P-256 point, as a browser's would be.
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    let key =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng).unwrap();
    let (endpoint, received) = fake_push_service();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    write_private(
        &state.join("mobile.json"),
        &json!({"enabled": true, "push_mode": "always"}),
    );
    write_private(
        &state.join("phones.json"),
        &json!({"phones": [{
            "id": "phone-1", "name": "test phone", "credential_sha256": "0".repeat(64),
            "paired_at_ms": now, "last_seen_ms": now, "notifications": "on",
            "push": {
                "endpoint": endpoint,
                "p256dh": URL_SAFE_NO_PAD.encode(key.public_key().as_ref()),
                "auth": URL_SAFE_NO_PAD.encode([7_u8; 16]),
            },
        }]}),
    );
    let running = hided::start_daemon(env(root.path(), None))
        .await
        .expect("hided starts");
    // The delivery pass runs on a blocking thread; the hang guard only ends
    // a daemon that never sends.
    let request =
        tokio::task::spawn_blocking(move || received.recv_timeout(Duration::from_secs(60)))
            .await
            .unwrap()
            .expect("the phone's push service received the notice");
    assert!(request.starts_with("POST /push/1 "), "{request}");
    let log =
        std::fs::read_to_string(root.path().join("state/Logs/core.jsonl")).unwrap_or_default();
    assert!(
        !log.contains("human.channels_failed"),
        "a notice that reached a phone is not a failure: {log}"
    );
    running.stop();
}
