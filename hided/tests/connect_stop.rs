//! `hide connect`, `hide status --json` and `hide stop` on every system: the
//! CLI starts the `hided` beside its own file, sees it answer `/health`, and
//! stops it. The desktop app meets the daemon only through these commands, so
//! a CLI that cannot find or ask its daemon on one system leaves that
//! system's app on its failure page. Each process runs on a private home and
//! state folder with no Herdr or Hide variables, so nothing reaches a live
//! Herdr or daemon.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

/// `hide` with the account's places moved under `home`; on Windows the home
/// and the folders Herdr's default pipe and the folder locks resolve from.
fn isolated(home: &Path, state: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hide"));
    for (key, _) in std::env::vars_os() {
        let upper = key.to_string_lossy().to_uppercase();
        if upper.starts_with("HERDR_") || upper.starts_with("HIDE_") {
            command.env_remove(key);
        }
    }
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("APPDATA", home.join("AppData").join("Roaming"))
        .env("LOCALAPPDATA", home.join("AppData").join("Local"))
        .env("XDG_CONFIG_HOME", home.join("xdg-config"))
        .env_remove("XDG_STATE_HOME")
        .env("HIDE_STATE_DIR", state)
        .env("HIDE_TAILSCALE_BIN", home.join("no-tailscale"))
        .stdin(Stdio::null())
        .stderr(Stdio::inherit());
    command
}

/// Runs `hide` and reads its output to the end, bounded: on Windows a
/// daemon that inherited the CLI's output pipe would keep it open for its
/// whole life, which a caller sees as a `hide` that never answers.
fn run(command: &mut Command) -> (bool, Vec<u8>) {
    let mut child = command.stdout(Stdio::piped()).spawn().expect("hide starts");
    let mut stdout = child.stdout.take().unwrap();
    let (done, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        let _ = done.send(bytes);
    });
    let bytes = heard
        .recv_timeout(Duration::from_secs(30))
        .expect("hide's output did not end within 30 s: a process it started holds it open");
    (child.wait().expect("hide ends").success(), bytes)
}

fn json(command: &mut Command) -> (bool, Value) {
    let (ok, stdout) = run(command);
    let line = serde_json::from_slice(&stdout).unwrap_or_else(|error| {
        panic!(
            "hide printed no JSON line ({error}): {}",
            String::from_utf8_lossy(&stdout)
        )
    });
    (ok, line)
}

/// Stops whatever daemon the test started, however it ends.
struct StopOnDrop<'a> {
    home: &'a Path,
    state: &'a Path,
}

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        let _ = isolated(self.home, self.state).arg("stop").status();
    }
}

#[test]
fn connect_starts_the_daemon_beside_the_cli_and_stop_ends_it() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    let _stop = StopOnDrop {
        home: &home,
        state: &state,
    };

    let (ok, connected) = json(isolated(&home, &state).arg("connect"));
    assert!(ok, "hide connect failed: {connected}");
    assert_eq!(connected["ok"], true, "{connected}");
    let pid = connected["pid"].as_u64().expect("a daemon pid") as u32;
    let started = hide_platform::process::start_time(pid).expect("the daemon is running");
    let port = connected["port"].as_u64().expect("a daemon port");
    assert!(
        connected["url"]
            .as_str()
            .is_some_and(|url| url.starts_with(&format!("http://127.0.0.1:{port}/"))),
        "the URL names the daemon's loopback port: {connected}"
    );

    let (_, running) = json(isolated(&home, &state).args(["status", "--json"]));
    assert_eq!(running["running"], true, "{running}");
    assert_eq!(
        running["pid"], connected["pid"],
        "status names the daemon connect started"
    );

    // `hide stop` answers once the daemon has ended, so its pid no longer
    // names the process that started then.
    let (stopped, _) = run(isolated(&home, &state).arg("stop"));
    assert!(stopped, "hide stop failed");
    assert_ne!(
        hide_platform::process::start_time(pid).ok(),
        Some(started),
        "hide stop leaves no daemon running"
    );
    let (_, after) = json(isolated(&home, &state).args(["status", "--json"]));
    assert_eq!(after["running"], false, "{after}");
}
