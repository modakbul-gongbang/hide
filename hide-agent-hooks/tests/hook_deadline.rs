use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Execs the hook and the sibling `hide` it runs once, untimed. macOS checks
/// a just-linked binary on its first exec: after a relink the 131 MB debug
/// `hide` took 1.8 s to start on an idle machine and 5 ms on the next exec,
/// which under a full workspace test run pushed the hook past the deadline
/// below. That one-time cost is not the hook's, so it is paid here.
fn warm_first_exec(hook: &Path) {
    let sibling = hook.parent().map(|dir| dir.join("hide"));
    for program in std::iter::once(hook.to_path_buf()).chain(sibling.filter(|path| path.exists())) {
        let _ = Command::new(program)
            .arg("--help")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[test]
fn blocked_stdin_cannot_hold_the_agent_hook_past_its_hard_deadline() {
    let home = tempfile::tempdir().unwrap();
    let hook = Path::new(env!("CARGO_BIN_EXE_hide-agent-hooks"));
    warm_first_exec(hook);
    let mut child = Command::new(hook)
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
