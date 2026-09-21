use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn blocked_stdin_cannot_hold_the_agent_hook_past_its_hard_deadline() {
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hide-agent-hooks"))
        .args([
            "hook",
            "--runtime",
            "claude-code",
            "--event",
            "SessionStart",
        ])
        .env("HOME", home.path())
        .env_remove("HERDR_PANE_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let held_open = child.stdin.take().unwrap();
    let started = Instant::now();
    let deadline = started + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            drop(held_open);
            let output = child.wait_with_output().unwrap();
            panic!(
                "hook exceeded the hard deadline; stdout={:?} stderr={:?}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    drop(held_open);
    let output = child.wait_with_output().unwrap();
    assert!(status.success());
    assert!(output.stderr.is_empty());
    let payload: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        payload["hookSpecificOutput"]["additionalContext"],
        hide_agent_hooks::runtime::PURPOSE_CONTEXT
    );
    // This target is an unoptimised Cargo test binary, so its process startup
    // is not the production hook timing surface. The signed release helper's
    // caller-visible 100 ms boundary is measured in native acceptance.
    assert!(started.elapsed() < Duration::from_millis(1_500));
}
