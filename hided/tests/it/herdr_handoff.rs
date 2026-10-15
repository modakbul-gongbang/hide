//! A Herdr server an older Hide app started moves onto this app's Herdr when
//! `hide connect` looks at it (`hided::herdr_handoff`), and the process in
//! its pane goes on running; a server running a Herdr no Hide app holds is
//! left as it is.
//!
//! Real processes against the pinned Herdr, so the test is ignored by
//! default; run it with `--ignored` and the binary in `HIDE_E2E_HERDR_BIN`.
//! Herdr hands a server over on Unix only.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hide_platform::fs::identity::{canonical, file_id};
use hide_platform::process::{image_of, is_alive};
use hided::herdr_handoff::{Outcome, hand_off};
use serde_json::Value;

use crate::private_herdr::{PrivateHerdr, herdr_command};

#[test]
#[ignore = "needs the pinned Herdr: set HIDE_E2E_HERDR_BIN and run with --ignored"]
fn a_server_an_older_app_started_moves_onto_this_apps_herdr_with_its_panes() {
    let pinned = PathBuf::from(
        std::env::var_os("HIDE_E2E_HERDR_BIN").expect("HIDE_E2E_HERDR_BIN names the pinned herdr"),
    );
    // Herdr makes its handoff socket inside its home, and macOS holds a
    // socket path to 104 bytes, which a folder under $TMPDIR outgrows.
    let root = tempfile::Builder::new()
        .prefix("hho")
        .tempdir_in("/tmp")
        .unwrap();
    let old = root.path().join("Old.app/Contents/Resources");
    let new = root.path().join("New.app/Contents/Resources");
    for resources in [&old, &new] {
        std::fs::create_dir_all(resources).unwrap();
        copy_program(&pinned, &resources.join("herdr"));
    }
    package_runtime(&new);
    let new_herdr = new.join("herdr");
    let mut herdr = PrivateHerdr::start(old.join("herdr"), &root.path().join("server"));

    let created = run(&herdr, &["workspace", "create", "--cwd"], &herdr.home);
    let pane = created["result"]["root_pane"]["pane_id"]
        .as_str()
        .expect("the new workspace names its pane")
        .to_owned();
    let pid_file = root.path().join("pane.pid");
    let command = format!("echo $$ > {}; exec sleep 600", pid_file.display());
    run(&herdr, &["pane", "run", &pane], Path::new(&command));
    let pane_pid = wait_for_pid(&pid_file);

    // A Herdr in a folder no Hide app made is the operator's own.
    assert_eq!(
        hand_off(&new_herdr, &herdr.socket),
        Outcome::NotHides {
            program: canonical(&old.join("herdr")).unwrap()
        }
    );

    // The old app was a Hide app, renamed aside as installing a newer one
    // does.
    package_runtime(&old);
    let aside = root.path().join("Old.app.bak");
    std::fs::rename(root.path().join("Old.app"), &aside).unwrap();
    let outcome = hand_off(&new_herdr, &herdr.socket);
    let Outcome::HandedOff {
        to_pid,
        from_program,
        ..
    } = outcome
    else {
        panic!("the server was not handed off: {outcome:?}");
    };
    // From here the server is this app's Herdr's to stop.
    herdr.bin = new_herdr.clone();
    assert_eq!(
        from_program,
        canonical(&aside.join("Contents/Resources/herdr")).unwrap()
    );
    assert_eq!(image_of(to_pid).unwrap().file, file_id(&new_herdr).unwrap());
    assert!(
        is_alive(pane_pid),
        "the pane's process ended with the old server"
    );
    let panes = run(&herdr, &["pane", "list"], Path::new(""));
    assert!(
        panes["result"]["panes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|listed| listed["pane_id"] == pane.as_str()),
        "the pane is gone from the new server: {panes}"
    );
    assert_eq!(hand_off(&new_herdr, &herdr.socket), Outcome::Ours);

    herdr.stop().unwrap();
    wait_until("the handed-off server ends", || !is_alive(to_pid));
}

/// Runs `herdr <args> [last]` against the private server and reads its JSON
/// answer; an empty `last` adds nothing.
fn run(herdr: &PrivateHerdr, args: &[&str], last: &Path) -> Value {
    let mut command = herdr_command(&herdr.bin, &herdr.home, &herdr.socket, &herdr.config);
    command.args(args);
    if !last.as_os_str().is_empty() {
        command.arg(last);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "herdr {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or(Value::Null)
}

/// What makes a folder a Hide app's runtime: its daemon beside the shell
/// archive (`hide_kit::holds_package_runtime`).
fn package_runtime(resources: &Path) {
    std::fs::write(resources.join("app.asar"), "").unwrap();
    std::fs::write(resources.join("hided"), "").unwrap();
}

/// A copy of `from` at `to` that runs as a program of its own, its bytes
/// written rather than cloned (`docs/TESTING.md`, Writing a Rust test, 8).
fn copy_program(from: &Path, to: &Path) {
    let mut source = std::fs::File::open(from).unwrap();
    let mut target = std::fs::File::create(to).unwrap();
    std::io::copy(&mut source, &mut target).unwrap();
    target
        .set_permissions(source.metadata().unwrap().permissions())
        .unwrap();
}

fn wait_for_pid(file: &Path) -> u32 {
    let mut pid = None;
    wait_until("the pane's shell writes its pid", || {
        pid = std::fs::read_to_string(file)
            .ok()
            .and_then(|text| text.trim().parse().ok());
        pid.is_some()
    });
    pid.unwrap()
}

/// Polls `done` until it holds; the minute only ends a wait that never
/// would.
#[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}
