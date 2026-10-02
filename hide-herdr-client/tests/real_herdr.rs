//! The client against the pinned Herdr itself, on the operating system the
//! test runs on: a request, a snapshot and a live event subscription over the
//! platform's local stream (a Unix socket, or a named pipe on Windows).
//!
//! It needs the pinned binary, so it is ignored by default; the `os contract`
//! lane runs it with `--ignored` on every system and names the binary in
//! `HIDE_E2E_HERDR_BIN`. The server it starts is private: its own socket,
//! session, config and home, none of the operator's.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use hide_herdr_client::{request_with_timeout, subscribe};
use serde_json::{Value, json};

fn pinned_version() -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../contracts/herdr-bundle.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    manifest["version"].as_str().unwrap().to_owned()
}

fn herdr_command(bin: &Path, home: &Path, socket: &Path, config: &Path) -> Command {
    let mut command = Command::new(bin);
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy().to_uppercase();
        if key.starts_with("HERDR_") || key.starts_with("HIDE_") {
            command.env_remove(key);
        }
    }
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join("xdg-config"))
        .env("XDG_STATE_HOME", home.join("xdg-state"))
        .env("HERDR_SESSION", "hide-contract")
        .env("HERDR_SOCKET_PATH", socket)
        .env("HERDR_CONFIG_PATH", config)
        .env("HERDR_DISABLE_SOUND", "1");
    command
}

struct PrivateHerdr {
    bin: PathBuf,
    home: PathBuf,
    socket: PathBuf,
    config: PathBuf,
    server: Child,
    // Dropped last: it holds every path above.
    _root: tempfile::TempDir,
}

impl PrivateHerdr {
    fn start(bin: PathBuf) -> Self {
        let version = String::from_utf8(
            Command::new(&bin)
                .arg("--version")
                .output()
                .expect("herdr --version")
                .stdout,
        )
        .unwrap();
        assert_eq!(
            version.split_whitespace().last().unwrap(),
            pinned_version(),
            "{} is not the pinned Herdr",
            bin.display()
        );
        // A Unix socket path is limited to about a hundred bytes.
        let root = if cfg!(unix) {
            tempfile::Builder::new().prefix("hr").tempdir_in("/tmp")
        } else {
            tempfile::Builder::new().prefix("hr").tempdir()
        }
        .unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let config = root.path().join("herdr-config.toml");
        std::fs::write(
            &config,
            "[update]\nversion_check = false\nmanifest_check = false\n",
        )
        .unwrap();
        let socket = root.path().join("herdr.sock");
        let log = std::fs::File::create(root.path().join("server.log")).unwrap();
        let server = herdr_command(&bin, &home, &socket, &config)
            .arg("server")
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("herdr server starts");
        let herdr = Self {
            bin,
            home,
            socket,
            config,
            server,
            _root: root,
        };
        let deadline = Instant::now() + Duration::from_secs(60);
        while request_with_timeout(&herdr.socket, "ping", json!({}), Duration::from_secs(2))
            .is_err()
        {
            assert!(Instant::now() < deadline, "the server never answered ping");
            thread::sleep(Duration::from_millis(250));
        }
        herdr
    }

    fn cli(&self, args: &[&str]) -> std::process::Output {
        herdr_command(&self.bin, &self.home, &self.socket, &self.config)
            .args(args)
            .output()
            .expect("herdr cli runs")
    }
}

impl Drop for PrivateHerdr {
    fn drop(&mut self) {
        let _ = self.cli(&["server", "stop"]);
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

#[test]
#[ignore = "needs the pinned Herdr: set HIDE_E2E_HERDR_BIN and run with --ignored"]
fn the_pinned_herdr_answers_a_request_and_streams_events_over_the_local_stream() {
    let bin = PathBuf::from(
        std::env::var_os("HIDE_E2E_HERDR_BIN").expect("HIDE_E2E_HERDR_BIN names the pinned herdr"),
    );
    let herdr = PrivateHerdr::start(bin);

    let pong = request_with_timeout(&herdr.socket, "ping", json!({}), Duration::from_secs(5))
        .expect("ping");
    assert_eq!(pong["type"], "pong");

    let snapshot = request_with_timeout(
        &herdr.socket,
        "session.snapshot",
        json!({}),
        Duration::from_secs(10),
    )
    .expect("session.snapshot");
    assert_eq!(snapshot["type"], "session_snapshot");

    let subscription = subscribe(
        &herdr.socket,
        &["workspace.created"],
        Duration::from_secs(5),
    )
    .expect("subscribe");
    assert_eq!(subscription.ack.kind, "subscription_started");
    let shutdown_after = {
        let (mut reader, shutdown) = subscription.into_parts();
        let (lines, heard) = mpsc::channel();
        thread::spawn(move || {
            use std::io::BufRead;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = lines.send(None);
                        return;
                    }
                    Ok(_) => {
                        let _ = lines.send(Some(line));
                    }
                }
            }
        });

        let created = herdr.cli(&[
            "workspace",
            "create",
            "--cwd",
            herdr.home.to_str().unwrap(),
            "--label",
            "contract",
        ]);
        assert!(
            created.status.success(),
            "workspace create failed: {}",
            String::from_utf8_lossy(&created.stderr)
        );
        let line = heard
            .recv_timeout(Duration::from_secs(30))
            .expect("an event arrived over the subscription")
            .expect("the subscription ended before an event");
        let event: Value = serde_json::from_str(&line).expect("event JSON");
        assert!(
            event["event"]
                .as_str()
                .is_some_and(|name| name.contains("workspace")),
            "unexpected event: {line}"
        );

        // The reader is blocked on the stream now; the handle must free it.
        let started = Instant::now();
        shutdown.shutdown();
        loop {
            match heard.recv_timeout(Duration::from_secs(5)) {
                Ok(None) => break,
                Ok(Some(_)) => continue,
                Err(_) => panic!("shutdown did not end the blocked read"),
            }
        }
        started.elapsed()
    };
    assert!(
        shutdown_after < Duration::from_secs(5),
        "{shutdown_after:?}"
    );
}
