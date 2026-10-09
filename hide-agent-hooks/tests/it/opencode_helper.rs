//! `hide-agent-hooks opencode <operation>`, what Hide's OpenCode plugin asks
//! (PRD opencode-plugin B3, B4, B7, B9, B11, B19), and the Memory Pi's and
//! omp's extension asks for (PRD pi-omp-extension B10): the compiled helper
//! beside a stand-in `hide` that records every call, run with the JSON the
//! script sends.

#![cfg(unix)]

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::{programs, stand_ins};

const PANE: &str = "w1:p1";

/// The `hide` the helper asks: records each call beside the test's HOME and
/// answers as Hide does for this pane.
const FAKE_HIDE: &str = r#"#!/bin/sh
echo "$@" >> "${HOME%/*}/hide-calls"
case "$1 $2" in
  "inbox --hook") echo '{"ok":true,"result":{"context":"LETTER-FOR-THIS-PANE","ids":["letter-1"],"remaining":0}}' ;;
  "inbox --confirm") echo '{"ok":true,"result":{"confirmed":["letter-1"]}}' ;;
  "workspace bootstrap") echo '{"ok":true}' ;;
  "workspace factory-question-guard") echo '{"type":"workspace_result","ok":true,"result":{"deny":true}}' ;;
  *) exit 1 ;;
esac
"#;

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
        stand_ins::place(programs::hook(), &helper);
        // The stand-in finds its record from HOME, which the helper hands on.
        stand_ins::program(&bin.join("hide"), FAKE_HIDE);
        Self {
            _dir: dir,
            calls: root.join("hide-calls"),
            home,
            helper,
        }
    }

    fn run(&self, operation: &str, input: &Value, in_pane: bool) -> Value {
        self.run_as("opencode", operation, input, in_pane)
    }

    fn run_as(&self, agent: &str, operation: &str, input: &Value, in_pane: bool) -> Value {
        self.run_bytes_as(agent, operation, input.to_string().as_bytes(), in_pane)
    }

    fn run_bytes(&self, operation: &str, input: &[u8], in_pane: bool) -> Value {
        self.run_bytes_as("opencode", operation, input, in_pane)
    }

    fn run_bytes_as(&self, agent: &str, operation: &str, input: &[u8], in_pane: bool) -> Value {
        let mut command = Command::new(&self.helper);
        command
            .args([agent, operation])
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
    // An id that would read as an option, or more than a hook's five, asks nothing.
    for letters in [
        json!(["--hook"]),
        json!(["l1", "l2", "l3", "l4", "l5", "l6"]),
    ] {
        assert_eq!(
            machine.run("confirm", &json!({ "letters": letters }), true),
            json!({})
        );
    }
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

/// A Memory-enabled Project beside the machine's HOME, in the store the
/// helper reads.
fn memory_project(machine: &Machine) -> (PathBuf, String, hide_memory::MemoryStore) {
    let root = machine.home.parent().unwrap().join("project");
    std::fs::create_dir_all(&root).unwrap();
    let node = hide_platform::host::machine_id().unwrap();
    let project = hide_project::resolve(&root, &node).unwrap();
    let database = hide_agent_hooks::memory::database_path(&machine.home);
    std::fs::create_dir_all(database.parent().unwrap()).unwrap();
    let store = hide_memory::MemoryStore::open(&database).unwrap();
    store
        .ensure_project(&project.id, &project.root, &node)
        .unwrap();
    store.set_enabled(&project.id, true, true).unwrap();
    (root, project.id, store)
}

/// The `event`, `items` and `auth` of the receipt line in `context`.
fn receipt(context: &str) -> Option<(String, String, String)> {
    let line = context
        .lines()
        .find(|line| line.starts_with("<hide-memory-receipt "))?;
    let field = |name: &str| {
        let start = line.find(&format!("{name}=\""))? + name.len() + 2;
        Some(line[start..].split_once('"')?.0.to_owned())
    };
    Some((field("event")?, field("items")?, field("auth")?))
}

/// Pi's and omp's extension names the session by its file for letters and by
/// the host's own id for Memory: the receipt is signed for the id, which is
/// what their session reader keys it by, never for the file; with no usable id
/// the prompt carries no Memory but still its letters. OpenCode's session id
/// is its own.
#[test]
fn pi_and_omp_memory_receipts_are_signed_for_the_hosts_session_id_and_opencodes_for_its_own() {
    let machine = Machine::new();
    let (project, project_id, store) = memory_project(&machine);
    let version = hide_agent_hooks::pi_extension::VERSION;
    for agent in ["pi", "omp"] {
        let file = format!("/sessions/-work-/2026-10-09T00-00-00-000Z_{agent}.jsonl");
        let id = format!("01a11d1d-{agent}");
        let answer = machine.run_as(
            agent,
            "prompt",
            &json!({"session_id": file, "native_session": id, "prompt": "Fix it", "cwd": project,
                "first": true, "memory_first": true, "version": version}),
            true,
        );
        assert_eq!(answer["memory_start"], json!(true), "{agent}");
        assert_eq!(answer["letters"], json!(["letter-1"]), "{agent}");
        let (event, items, auth) = receipt(answer["context"].as_str().unwrap()).expect(agent);
        assert_eq!(event, "SessionStart");
        let verifies = |session: &str| {
            store
                .verify_receipt_auth(&project_id, agent, session, &event, &items, &auth)
                .unwrap()
        };
        assert!(verifies(&id), "{agent}: signed for the host's id");
        assert!(!verifies(&file), "{agent}: never for the session file");

        // Once the start capsule is written, the prompt asks for the prompt
        // capsule, which is no session start.
        let answer = machine.run_as(
            agent,
            "prompt",
            &json!({"session_id": file, "native_session": id, "prompt": "Fix it", "cwd": project,
                "first": true, "memory_first": false, "version": version}),
            true,
        );
        assert_eq!(answer["memory_start"], json!(false), "{agent}");

        // An id that would read as an option is no session: no Memory, the letters still ride.
        let answer = machine.run_as(
            agent,
            "prompt",
            &json!({"session_id": file, "native_session": "-x", "prompt": "Fix it", "cwd": project,
                "first": true, "memory_first": true, "version": version}),
            true,
        );
        assert_eq!(answer["memory_start"], json!(false), "{agent}");
        assert_eq!(answer["letters"], json!(["letter-1"]), "{agent}");
        assert!(
            receipt(answer["context"].as_str().unwrap()).is_none(),
            "{agent}"
        );
    }

    // With Memory off no capsule is given, so the extension asks again.
    store.set_enabled(&project_id, false, true).unwrap();
    let answer = machine.run_as(
        "pi",
        "prompt",
        &json!({"session_id": "/sessions/-work-/off.jsonl", "native_session": "01a11d1d-off",
            "prompt": "Fix it", "cwd": project, "first": true, "memory_first": true, "version": version}),
        true,
    );
    assert_eq!(answer["memory_start"], json!(false));
    store.set_enabled(&project_id, true, true).unwrap();

    let answer = machine.run(
        "prompt",
        &json!({"session_id": "ses_root", "prompt": "Fix it", "cwd": project, "first": true}),
        true,
    );
    let (event, items, auth) = receipt(answer["context"].as_str().unwrap()).unwrap();
    assert!(
        store
            .verify_receipt_auth(&project_id, "opencode", "ses_root", &event, &items, &auth)
            .unwrap()
    );
}
