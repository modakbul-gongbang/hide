//! `hide-agent-hooks opencode <operation>`, what Hide's OpenCode plugin asks
//! (PRD opencode-plugin B3, B4, B7, B9, B11, B19): the compiled helper beside a
//! stand-in `hide` that records every call, run with the JSON the plugin sends.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const PANE: &str = "w1:p1";

struct Machine {
    _dir: tempfile::TempDir,
    home: PathBuf,
    helper: PathBuf,
    calls: PathBuf,
}

impl Machine {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let home = root.join("home");
        let bin = root.join("bin");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let helper = bin.join("hide-agent-hooks");
        std::fs::copy(env!("CARGO_BIN_EXE_hide-agent-hooks"), &helper).unwrap();
        let calls = root.join("hide-calls");
        let fake = bin.join("hide");
        std::fs::write(
            &fake,
            format!(
                r#"#!/bin/sh
echo "$@" >> '{calls}'
case "$1 $2" in
  "inbox --hook") echo '{{"ok":true,"result":{{"context":"LETTER-FOR-THIS-PANE","ids":["letter-1"],"remaining":0}}}}' ;;
  "inbox --confirm") echo '{{"ok":true,"result":{{"confirmed":["letter-1"]}}}}' ;;
  "workspace bootstrap") echo '{{"ok":true}}' ;;
  "workspace factory-question-guard") echo '{{"type":"workspace_result","ok":true,"result":{{"deny":true}}}}' ;;
  *) exit 1 ;;
esac
"#,
                calls = calls.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let machine = Self {
            _dir: dir,
            home,
            helper,
            calls,
        };
        // A copied executable's first run pays macOS's validation; both are
        // paid here, outside the operations' budgets.
        machine.run("warm", &json!({}), true);
        Command::new(&fake).arg("warm").output().unwrap();
        std::fs::remove_file(&machine.calls).ok();
        machine
    }

    fn run(&self, operation: &str, input: &Value, in_pane: bool) -> Value {
        self.run_bytes(operation, input.to_string().as_bytes(), in_pane)
    }

    fn run_bytes(&self, operation: &str, input: &[u8], in_pane: bool) -> Value {
        let mut command = Command::new(&self.helper);
        command
            .args(["opencode", operation])
            .env(hide_platform::host::HOME_VARIABLE, &self.home)
            .env("PATH", "/usr/bin:/bin")
            .env("HERDR_SOCKET_PATH", self.home.join("no-herdr.sock"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy();
            if ["HERDR_PANE_ID", "HERDR_ENV", "HIDE_", "HCOORD_"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
            {
                command.env_remove(key);
            }
        }
        if in_pane {
            command.env("HERDR_PANE_ID", PANE).env("HERDR_ENV", "1");
        }
        // Every operation ends within its own deadline, which is what bounds this wait.
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{operation} exits 0");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.calls)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

#[test]
fn a_prompt_takes_the_panes_letters_for_this_session_and_confirms_nothing() {
    let machine = Machine::new();
    let answer = machine.run(
        "prompt",
        &json!({"session_id": "ses_root", "prompt": "Fix it", "cwd": machine.home, "first": true}),
        true,
    );
    assert_eq!(answer["letters"], json!(["letter-1"]));
    assert!(
        answer["context"]
            .as_str()
            .unwrap()
            .contains("LETTER-FOR-THIS-PANE")
    );
    assert_eq!(machine.calls(), ["inbox --hook --session ses_root"]);
}

#[test]
fn confirm_confirms_exactly_the_letters_the_plugin_saw_stored() {
    let machine = Machine::new();
    let answer = machine.run("confirm", &json!({"letters": ["letter-1"]}), true);
    assert_eq!(answer["confirmed"], json!(["letter-1"]));
    assert_eq!(machine.calls(), ["inbox --confirm letter-1"]);
    // Nothing to confirm asks nothing.
    let machine = Machine::new();
    assert_eq!(
        machine.run("confirm", &json!({"letters": []}), true),
        json!({})
    );
    assert!(machine.calls().is_empty());
}

#[test]
fn a_shell_call_that_starts_an_agent_through_herdr_is_refused_with_the_spawn_guards_reason() {
    let machine = Machine::new();
    let answer = machine.run(
        "tool",
        &json!({"session_id": "ses_root", "tool": "bash",
                "command": "herdr agent start --kind claude --name helper", "cwd": machine.home}),
        true,
    );
    let reason = answer["deny"].as_str().unwrap();
    assert!(
        reason.contains("hide agent spawn --parent here"),
        "{reason}"
    );
    assert!(reason.contains("--kind claude"), "{reason}");
    assert_eq!(machine.calls(), ["workspace bootstrap"]);
    let log =
        std::fs::read_to_string(machine.home.join(".hide/agent-hooks/spawn-guard.log")).unwrap();
    assert!(log.contains("\"runtime\":\"opencode\""), "{log}");
    assert!(
        !log.contains("--name helper"),
        "the command is never logged"
    );

    // Any other shell call runs without asking.
    let machine = Machine::new();
    let answer = machine.run(
        "tool",
        &json!({"session_id": "ses_root", "tool": "bash", "command": "cargo test"}),
        true,
    );
    assert_eq!(answer, json!({}));
    assert!(machine.calls().is_empty());
}

#[test]
fn the_question_tool_asks_the_factory_guard_as_opencode() {
    let machine = Machine::new();
    let answer = machine.run(
        "tool",
        &json!({"session_id": "ses_root", "tool": "question"}),
        true,
    );
    assert_eq!(
        answer["deny"],
        hide_agent_hooks::spawn_guard::QUESTION_REASON
    );
    assert_eq!(
        machine.calls(),
        ["workspace factory-question-guard --session ses_root --runtime opencode"]
    );
}

#[test]
fn outside_a_pane_or_with_unreadable_input_the_helper_answers_nothing_and_asks_nothing() {
    let machine = Machine::new();
    assert_eq!(
        machine.run(
            "tool",
            &json!({"session_id": "ses_root", "tool": "question"}),
            false
        ),
        json!({})
    );
    assert_eq!(
        machine.run("subagents", &json!({"working": 1, "done": 0}), false),
        json!({})
    );
    assert_eq!(machine.run_bytes("prompt", b"not json", true), json!({}));
    assert_eq!(machine.run("unknown", &json!({}), true), json!({}));
    assert!(machine.calls().is_empty());
}

#[test]
fn subagent_counts_are_stored_as_sent_and_a_failed_report_is_recorded() {
    let machine = Machine::new();
    let answer = machine.run("subagents", &json!({"working": 2, "done": 1}), true);
    assert_eq!(answer, json!({"reported": false}));
    let stored: Value = serde_json::from_slice(
        &std::fs::read(machine.home.join(".hide/agent-hooks/panes/w1_p1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(stored, json!({"working": 2, "done": 1}));
    let failure = hide_agent_hooks::report::last_failure(&machine.home).unwrap();
    assert_eq!(failure.pane_id, PANE);
    assert_eq!(failure.event, "subagent count");
}

#[test]
fn start_answers_hides_session_guidance() {
    let machine = Machine::new();
    let answer = machine.run("start", &json!({"cwd": machine.home}), true);
    let context = answer["context"].as_str().unwrap();
    assert!(
        context.contains(hide_agent_hooks::guidance::GUIDANCE_LINE),
        "{context}"
    );
}
