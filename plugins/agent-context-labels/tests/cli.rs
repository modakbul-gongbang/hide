//! Drives the built binary against a temporary home, so what a command does
//! to the state directories is observed the way an operator would see it.
//! Nothing here reads or writes the real home.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use agent_context_labels::{LEGACY_PLUGIN_ID, PLUGIN_ID};
use tempfile::{TempDir, tempdir};

const BIN: &str = env!("CARGO_BIN_EXE_hide-agent-context-labels");

fn legacy_state(home: &Path) -> PathBuf {
    home.join(".local/state").join(LEGACY_PLUGIN_ID)
}

fn current_state(home: &Path) -> PathBuf {
    home.join(".local/state").join(PLUGIN_ID)
}

/// A home with only the previous id's state directory in it.
fn home_with_legacy_state() -> TempDir {
    let home = tempdir().unwrap();
    let legacy = legacy_state(home.path());
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(
        legacy.join("settings.json"),
        r#"{"automatic_summaries":false}"#,
    )
    .unwrap();
    home
}

/// The binary with an empty PATH, so no Herdr and no Codex is ever found.
fn command(home: &Path) -> Command {
    let mut command = Command::new(BIN);
    command
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[test]
fn commands_other_than_watch_leave_legacy_state_where_it_is() {
    let cases: &[&[&str]] = &[
        &["--help"],
        &["not-a-command"],
        &["request-refresh"],
        &["set-automatic-summaries", "--enabled", "true"],
        &["verify-provider", "--provider", "claude"],
        &["hook"],
    ];
    for args in cases {
        let home = home_with_legacy_state();
        let output = command(home.path()).args(*args).output().unwrap();
        assert!(
            legacy_state(home.path()).join("settings.json").exists(),
            "{args:?} moved the legacy state (status {:?}, stderr {})",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        let log = current_state(home.path()).join("events.jsonl");
        if log.exists() {
            let log = std::fs::read_to_string(log).unwrap();
            assert!(!log.contains("state_migrated"), "{args:?}: {log}");
        }
    }
}

#[test]
fn help_and_an_invalid_command_create_no_state_at_all() {
    for args in [&["--help"][..], &["not-a-command"][..]] {
        let home = tempdir().unwrap();
        let _ = command(home.path()).args(args).output().unwrap();
        assert!(
            !home.path().join(".local").exists(),
            "{args:?} created state under the home"
        );
    }
}

#[test]
fn verify_provider_names_an_unsupported_provider_and_fails() {
    let home = tempdir().unwrap();
    let output = command(home.path())
        .args(["verify-provider", "--provider", "claude"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("claude=unsupported"), "{stdout}");
}

#[test]
fn the_watcher_moves_legacy_state_once_on_start() {
    let home = home_with_legacy_state();
    let mut watcher = command(home.path()).arg("watch").spawn().unwrap();
    let log = current_state(home.path()).join("events.jsonl");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = String::new();
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(&log)
            && text.contains("watcher_started")
        {
            seen = text;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = watcher.kill();
    let _ = watcher.wait();
    assert!(
        seen.contains("watcher_started"),
        "watcher never started: {seen}"
    );
    assert!(
        seen.contains(r#""event":"state_migrated""#) && seen.contains(".local/state=moved"),
        "{seen}"
    );
    assert!(!legacy_state(home.path()).exists());
    assert_eq!(
        std::fs::read_to_string(current_state(home.path()).join("settings.json")).unwrap(),
        r#"{"automatic_summaries":false}"#
    );
}
