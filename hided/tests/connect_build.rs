// Re-signing a copy to make a second build needs macOS codesign.
#![cfg(target_os = "macos")]

//! `hide connect` and the daemon's build (PRD labels-in-hided D-19, B26,
//! B27): a running daemon of another build is replaced, one of the same
//! build is attached to. Every process here runs on a private HOME and state
//! folder with no Herdr variables, so nothing reaches a live Herdr or daemon.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// A command with nothing inherited that could name a live Herdr, daemon or
/// pane.
fn isolated(program: &Path, home: &Path, state: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", home)
        .env("HIDE_STATE_DIR", state)
        .env("HIDE_TAILSCALE_BIN", home.join("no-tailscale"))
        .stdin(Stdio::null());
    command
}

fn try_connect(cli: &Path, home: &Path, state: &Path) -> (bool, Value) {
    let output: Output = isolated(cli, home, state)
        .arg("connect")
        .stderr(Stdio::inherit())
        .output()
        .expect("hide connect runs");
    let line = serde_json::from_slice(&output.stdout).expect("hide connect prints one JSON line");
    (output.status.success(), line)
}

fn connect(cli: &Path, home: &Path, state: &Path) -> Value {
    let (ok, line) = try_connect(cli, home, state);
    assert!(ok, "hide connect failed: {line}");
    line
}

fn state_pid(state: &Path) -> i64 {
    let bytes = std::fs::read(state.join("hided.json")).expect("a daemon state");
    serde_json::from_slice::<Value>(&bytes).unwrap()["pid"]
        .as_i64()
        .unwrap()
}

fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

fn wait_gone(pid: i32) -> bool {
    let until = Instant::now() + Duration::from_secs(10);
    while alive(pid) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    !alive(pid)
}

/// A copy of `hide` beside a `hided` whose bytes differ from the cargo-built
/// one only in its ad-hoc signature, as a package of the same source does.
/// `dir` decides whether it is the app's own: an app bundle's
/// `Contents/Resources`, or anywhere else.
fn other_build(dir: &Path) -> PathBuf {
    let built = Path::new(env!("CARGO_BIN_EXE_hided"));
    let cli = dir.join("hide");
    let daemon = dir.join("hided");
    std::fs::create_dir_all(dir).unwrap();
    std::fs::copy(built.with_file_name("hide"), &cli).unwrap();
    std::fs::copy(built, &daemon).unwrap();
    let signed = Command::new("/usr/bin/codesign")
        .args([
            "--force",
            "--sign",
            "-",
            "--identifier",
            "dev.hide.other-build",
        ])
        .arg(&daemon)
        .status()
        .unwrap();
    assert!(signed.success(), "codesign could not re-sign the copy");
    cli
}

/// Stops whatever daemon the state folder names when the test ends, also
/// after a failed assertion.
struct StopOnDrop<'a> {
    cli: &'a Path,
    home: &'a Path,
    state: &'a Path,
}

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        let _ = isolated(self.cli, self.home, self.state)
            .arg("stop")
            .status();
    }
}

#[test]
fn the_apps_hide_replaces_a_daemon_of_another_build_and_keeps_one_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    let cargo_cli = Path::new(env!("CARGO_BIN_EXE_hided")).with_file_name("hide");
    let _stop = StopOnDrop {
        cli: &cargo_cli,
        home: &home,
        state: &state,
    };

    let first = connect(&cargo_cli, &home, &state);
    let first_pid = first["pid"].as_i64().unwrap() as i32;
    let again = connect(&cargo_cli, &home, &state);
    assert_eq!(
        again["pid"], first["pid"],
        "the same build attaches to the running daemon"
    );

    let app = other_build(&dir.path().join("Hide.app/Contents/Resources"));
    let replaced = connect(&app, &home, &state);
    let replaced_pid = replaced["pid"].as_i64().unwrap() as i32;
    assert_ne!(
        replaced_pid, first_pid,
        "the app's build starts its own daemon"
    );
    assert!(
        wait_gone(first_pid),
        "the daemon of the other build is gone, not left beside the new one"
    );

    let _ = isolated(&cargo_cli, &home, &state).arg("stop").status();
    assert!(wait_gone(replaced_pid), "hide stop ends the replacement");
}

/// A `hide` outside an app bundle (a dev build, a copy) never stops the
/// daemon it finds; it names the mismatch and attaches to nothing.
#[test]
fn a_hide_outside_the_app_refuses_a_daemon_of_another_build() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    let cargo_cli = Path::new(env!("CARGO_BIN_EXE_hided")).with_file_name("hide");
    let _stop = StopOnDrop {
        cli: &cargo_cli,
        home: &home,
        state: &state,
    };
    let running = connect(&cargo_cli, &home, &state);

    let copy = other_build(&dir.path().join("copy"));
    let (ok, line) = try_connect(&copy, &home, &state);

    assert!(!ok, "{line}");
    assert_eq!(line["reason"], "other_build", "{line}");
    assert!(alive(running["pid"].as_i64().unwrap() as i32));
    assert_eq!(state_pid(&state), running["pid"].as_i64().unwrap());
}

/// Two app connects that both find a daemon of another build leave one
/// daemon, the one the state names, and both attach to it.
#[test]
fn two_connects_at_once_leave_one_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    let cargo_cli = Path::new(env!("CARGO_BIN_EXE_hided")).with_file_name("hide");
    let _stop = StopOnDrop {
        cli: &cargo_cli,
        home: &home,
        state: &state,
    };
    let old = connect(&cargo_cli, &home, &state)["pid"].as_i64().unwrap() as i32;
    let app = other_build(&dir.path().join("Hide.app/Contents/Resources"));

    let racers: Vec<_> = (0..2)
        .map(|_| {
            let (app, home, state) = (app.clone(), home.clone(), state.clone());
            std::thread::spawn(move || connect(&app, &home, &state))
        })
        .collect();
    let pids: Vec<i64> = racers
        .into_iter()
        .map(|racer| racer.join().unwrap()["pid"].as_i64().unwrap())
        .collect();

    assert!(wait_gone(old), "the old build's daemon is gone");
    assert_eq!(pids[0], pids[1], "both connects attach to one daemon");
    assert_eq!(state_pid(&state), pids[0]);
    // Every daemon of this state folder holds its instance lock open.
    let holders = Command::new("/usr/sbin/lsof")
        .arg("-t")
        .arg(state.join("hided.lock"))
        .output()
        .unwrap();
    let holders: Vec<i64> = String::from_utf8_lossy(&holders.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect();
    assert_eq!(holders, [pids[0]], "one daemon holds the instance lock");
    let _ = isolated(&cargo_cli, &home, &state).arg("stop").status();
    assert!(wait_gone(pids[0] as i32));
}
