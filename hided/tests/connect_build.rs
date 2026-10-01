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

fn connect(cli: &Path, home: &Path, state: &Path) -> Value {
    let output: Output = isolated(cli, home, state)
        .arg("connect")
        .stderr(Stdio::inherit())
        .output()
        .expect("hide connect runs");
    assert!(
        output.status.success(),
        "hide connect failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).expect("hide connect prints one JSON line")
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
fn connect_replaces_a_daemon_of_another_build_and_keeps_one_of_its_own() {
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

    let other = other_build(&dir.path().join("other"));
    let replaced = connect(&other, &home, &state);
    let replaced_pid = replaced["pid"].as_i64().unwrap() as i32;
    assert_ne!(
        replaced_pid, first_pid,
        "another build starts its own daemon"
    );
    assert!(
        wait_gone(first_pid),
        "the daemon of the other build is gone, not left beside the new one"
    );

    let _ = isolated(&cargo_cli, &home, &state).arg("stop").status();
    assert!(wait_gone(replaced_pid), "hide stop ends the replacement");
}
