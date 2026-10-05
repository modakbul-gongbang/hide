//! The guidance hook of the agents beyond Claude Code and Codex, run as the
//! agent runs it: the compiled helper, a private `HOME`, an empty `PATH`.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use hide_agent_hooks::guidance::{GUIDANCE_LINE, GuidanceAgent};
use hide_platform::process::OwnedChild;

fn run(home: &Path, runtime: &str, event: &str) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hide-agent-hooks"));
    command
        .args(["hook", "--runtime", runtime, "--event", event])
        .args(["--source", "hide-guidance@1"])
        .env(hide_platform::host::HOME_VARIABLE, home)
        // No `hide` to ask: the guidance must not need the daemon.
        .env("PATH", home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if ["HERDR_", "HIDE_", "HCOORD_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            command.env_remove(key);
        }
    }
    let mut child = OwnedChild::spawn(&mut command).unwrap();
    let output = child
        .capture_until(Instant::now() + Duration::from_secs(10), 64 * 1024)
        .expect("the guidance hook ends within its bound");
    assert!(output.status.success());
    assert!(output.stderr.is_empty(), "a hook says nothing on stderr");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn every_agent_gets_the_guidance_in_its_own_field_at_session_start() {
    let home = tempfile::tempdir().unwrap();
    for agent in GuidanceAgent::ALL {
        let stdout = run(home.path(), agent.id(), "SessionStart");
        assert!(stdout.contains("hide browser help"), "{agent:?}: {stdout}");
        match agent {
            GuidanceAgent::Kiro => assert!(stdout.starts_with("When you create a worktree")),
            GuidanceAgent::Copilot => {
                let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
                assert!(
                    value["additionalContext"]
                        .as_str()
                        .unwrap()
                        .contains(GUIDANCE_LINE)
                );
            }
            _ => {
                let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
                assert!(
                    value["hookSpecificOutput"]["additionalContext"]
                        .as_str()
                        .unwrap()
                        .contains(GUIDANCE_LINE)
                );
            }
        }
    }
}

#[test]
fn the_guidance_hook_prints_nothing_for_any_other_event_and_writes_no_pane_state() {
    let home = tempfile::tempdir().unwrap();
    for agent in GuidanceAgent::ALL {
        assert_eq!(run(home.path(), agent.id(), "Stop"), "", "{agent:?}");
    }
    assert!(
        std::fs::read_dir(home.path()).unwrap().next().is_none(),
        "the guidance hook counts nothing and keeps no file"
    );
}

#[test]
fn a_session_started_twice_gets_the_same_guidance_and_changes_no_state() {
    let home = tempfile::tempdir().unwrap();
    for agent in GuidanceAgent::ALL {
        let first = run(home.path(), agent.id(), "SessionStart");
        let second = run(home.path(), agent.id(), "SessionStart");
        assert_eq!(first, second, "{agent:?}");
    }
    assert!(std::fs::read_dir(home.path()).unwrap().next().is_none());
}
