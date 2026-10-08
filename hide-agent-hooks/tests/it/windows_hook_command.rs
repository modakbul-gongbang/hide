//! The Windows entries, started the way each runtime starts a hook there
//! (`docs/agent-hooks.md`, Installing): Claude Code spawns
//! the exec form's `command` with its `args` and no shell; Codex hands the
//! command to its session shell, PowerShell, as `<shell> -NoProfile -Command
//! <command>`. The helper is the real one, in a folder whose name has a
//! space, a quote, brackets and a `$`, so the quoting, the guard, the
//! arguments, stdin and the output all cross PowerShell as they will on an
//! operator's machine.
#![cfg(windows)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use hide_agent_hooks::{AgentRuntime, HELPER_BINARY_NAME, PURPOSE_CONTEXT};
use serde_json::Value;

/// Codex prefers PowerShell 7 and falls back to Windows PowerShell
/// (`codex-rs/shell-command/src/shell_detect.rs`); Windows PowerShell is on
/// every Windows, PowerShell 7 on the CI image.
fn codex_shells() -> Vec<&'static str> {
    let mut shells = vec!["powershell.exe"];
    let pwsh = Command::new("pwsh.exe")
        .args(["-NoProfile", "-Command", "exit 0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match pwsh {
        Ok(status) if status.success() => shells.push("pwsh.exe"),
        other => eprintln!(
            "PowerShell 7 is not here ({other:?}); Codex's entry runs under Windows PowerShell only"
        ),
    }
    shells
}

fn session_start_hook(home: &Path, runtime: AgentRuntime) -> Value {
    let raw = std::fs::read_to_string(runtime.config_path(home)).unwrap();
    let document: Value = serde_json::from_str(&raw).unwrap();
    document["hooks"]["SessionStart"][0]["hooks"][0].clone()
}

fn run(runtime: AgentRuntime, hook: &Value, shell: &str, home: &Path, cwd: &Path) -> Output {
    let mut command = match runtime {
        AgentRuntime::ClaudeCode => {
            let mut command = Command::new(hook["command"].as_str().unwrap());
            command.args(
                hook["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|argument| argument.as_str().unwrap()),
            );
            command
        }
        AgentRuntime::Codex => {
            let mut command = Command::new(shell);
            command.args(["-NoProfile", "-Command", hook["command"].as_str().unwrap()]);
            command
        }
    };
    let mut child = command
        .env(hide_platform::host::HOME_VARIABLE, home)
        .env_remove("HERDR_PANE_ID")
        .env_remove("HIDE_CAP_REF")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = serde_json::json!({
        "session_id": "windows-hook-command",
        "hook_event_name": "SessionStart",
        "cwd": cwd,
    });
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// A Project with Memory on and nothing in it: its SessionStart context
/// carries a receipt only when the helper read the session and the cwd from
/// stdin, which is how the test sees stdin cross the shell.
fn enable_memory(home: &Path, project_root: &Path) {
    // The hook keys this machine's Projects by its node id.
    let node = hide_platform::host::machine_id().unwrap();
    let project = hide_project::resolve(project_root, &node).unwrap();
    let path = hide_agent_hooks::memory::database_path(home);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let store = hide_memory::MemoryStore::open(&path).unwrap();
    store
        .ensure_project(&project.id, &project.root, &node)
        .unwrap();
    store.set_enabled(&project.id, true, true).unwrap();
}

const RECEIPT: &str = "<hide-memory-receipt event=\"SessionStart\"";

/// A debug helper on a shared runner can miss its 75 ms Memory budget and
/// answer without the receipt; without stdin it never carries one, so one
/// receipt in a few runs is the proof.
const ATTEMPTS: usize = 5;

#[test]
fn the_windows_entries_run_the_helper_through_powershell_only_while_it_is_there() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    for runtime in AgentRuntime::ALL {
        std::fs::create_dir_all(runtime.home_directory(&home)).unwrap();
    }
    // `[`, `]` and `$` are what PowerShell could take for a wildcard or a
    // variable outside a literal.
    let helper = home.join("it's [1] $x here").join(HELPER_BINARY_NAME);
    for runtime in AgentRuntime::ALL {
        hide_agent_hooks::install(runtime, &home, &helper).unwrap();
        assert_eq!(
            hide_agent_hooks::installed_helper_path(runtime, &home).as_deref(),
            helper.to_str(),
            "{runtime:?}: the path is read back from the PowerShell guard"
        );
    }
    let shells = codex_shells();
    let ways = |runtime: AgentRuntime| -> Vec<&'static str> {
        match runtime {
            AgentRuntime::ClaudeCode => vec!["exec form"],
            AgentRuntime::Codex => shells.clone(),
        }
    };

    // The app was moved away: every hook succeeds and says nothing (B3).
    for runtime in AgentRuntime::ALL {
        let hook = session_start_hook(&home, runtime);
        for how in ways(runtime) {
            let output = run(runtime, &hook, how, &home, &project);
            assert!(
                output.status.success() && output.stdout.is_empty(),
                "{runtime:?} under {how}: {output:?}"
            );
        }
    }

    std::fs::create_dir_all(helper.parent().unwrap()).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_hide-agent-hooks"), &helper).unwrap();
    enable_memory(&home, &project);
    for runtime in AgentRuntime::ALL {
        let hook = session_start_hook(&home, runtime);
        for how in ways(runtime) {
            let mut receipt = false;
            for _ in 0..ATTEMPTS {
                let output = run(runtime, &hook, how, &home, &project);
                let stdout = String::from_utf8_lossy(&output.stdout);
                assert!(
                    output.status.success(),
                    "{runtime:?} under {how}: {output:?}"
                );
                assert!(stdout.is_ascii(), "{runtime:?} under {how}: {stdout}");
                let envelope: Value = serde_json::from_str(&stdout)
                    .unwrap_or_else(|error| panic!("{runtime:?} under {how}: {error}: {stdout}"));
                assert_eq!(
                    envelope["hookSpecificOutput"]["hookEventName"], "SessionStart",
                    "{runtime:?} under {how}"
                );
                let context = envelope["hookSpecificOutput"]["additionalContext"]
                    .as_str()
                    .unwrap();
                assert!(
                    context.starts_with(PURPOSE_CONTEXT),
                    "{runtime:?} under {how}: the context's `…` crossed PowerShell intact: {context}"
                );
                if context.contains(RECEIPT) {
                    receipt = true;
                    break;
                }
            }
            assert!(
                receipt,
                "{runtime:?} under {how}: no run read the session from stdin"
            );
        }
    }
}
