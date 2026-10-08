//! Grok's and Cursor's own hooks (PRD grok-cursor-hooks B2 to B6, B13), run as
//! each agent runs them: the compiled helper with the agent's documented
//! payload on stdin, its documented environment, a stand-in `hide` that
//! answers the daemon's calls, and a stand-in Herdr socket that records the
//! pane reports. What it asserts is what the agent and Herdr observe: the
//! answer on stdout, and the counts Herdr is told.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::spawn_guard::{Machine, PANE, Run, START};

const GROK: &[(&str, &str)] = &[
    ("GROK_HOOK_EVENT", "pre_tool_use"),
    ("GROK_SESSION_ID", "grok-session"),
];

fn grok_shell(machine: &Machine, command: &str) -> String {
    json!({
        "hookEventName": "pre_tool_use",
        "hook_event_name": "PreToolUse",
        "sessionId": "grok-session",
        "cwd": machine.checkout,
        "workspaceRoot": machine.checkout,
        "permissionMode": "default",
        "toolName": "run_terminal_command",
        "toolInput": { "command": command },
        "toolUseId": "t1",
        "toolInputTruncated": false,
    })
    .to_string()
}

fn cursor_shell(machine: &Machine, command: &str) -> String {
    json!({
        "conversation_id": "c1",
        "generation_id": "g1",
        "hook_event_name": "preToolUse",
        "cursor_version": "2026.10.01",
        "workspace_roots": [machine.checkout],
        "tool_name": "Shell",
        "tool_input": { "command": command, "working_directory": machine.checkout },
        "tool_use_id": "abc123",
        "cwd": machine.checkout,
    })
    .to_string()
}

fn cursor_answer(run: &Run) -> Value {
    assert!(run.status_ok);
    serde_json::from_str(run.stdout.trim()).unwrap_or_else(|_| panic!("{:?}", run.stdout))
}

const CURSOR: &[(&str, &str)] = &[("CURSOR_VERSION", "2026.10.01")];

#[test]
fn a_grok_launch_is_refused_with_the_reason_claude_code_gets_and_other_calls_run() {
    let machine = Machine::new("registered");
    let run = machine.run_event(
        "grok",
        "PreToolUse",
        &grok_shell(&machine, START),
        Some(PANE),
        GROK,
    );
    assert!(run.status_ok);
    let reason = machine.reason(&run);
    assert!(
        reason.contains("hide agent spawn --parent here"),
        "{reason}"
    );
    assert!(reason.contains("--branch topic"), "{reason}");
    assert!(machine.guard_log().contains("\"runtime\":\"grok\""));

    let run = machine.run_event(
        "grok",
        "PreToolUse",
        &grok_shell(&machine, "cargo test"),
        Some(PANE),
        GROK,
    );
    assert!(run.status_ok);
    assert_eq!(run.stdout, "", "an ordinary call runs with no answer");
}

#[test]
fn a_cursor_launch_is_refused_in_cursors_shape_and_every_other_path_answers_allow() {
    let machine = Machine::new("registered");
    let run = machine.run_event(
        "cursor",
        "PreToolUse",
        &cursor_shell(&machine, START),
        Some(PANE),
        CURSOR,
    );
    let answer = cursor_answer(&run);
    assert_eq!(answer["permission"], "deny");
    let reason = answer["agent_message"].as_str().unwrap();
    assert!(
        reason.contains("hide agent spawn --parent here"),
        "{reason}"
    );
    assert_eq!(answer.as_object().unwrap().len(), 2, "{answer}");

    let allow = json!({ "permission": "allow" });
    for (payload, pane) in [
        (cursor_shell(&machine, "npm install"), Some(PANE)),
        // Outside a Herdr pane there is nothing to redirect (B6).
        (cursor_shell(&machine, START), None),
        // A payload the guard cannot read.
        ("herdr agent start but not json".to_owned(), Some(PANE)),
        (String::new(), Some(PANE)),
    ] {
        let run = machine.run_event("cursor", "PreToolUse", &payload, pane, CURSOR);
        assert_eq!(cursor_answer(&run), allow, "{payload}");
        assert_eq!(run.stderr, "", "{payload}");
    }
}

#[test]
fn a_cursor_guard_that_cannot_decide_still_answers_allow_within_its_budget() {
    for daemon in ["unregistered", "down", "mute", "later", "slow"] {
        let machine = Machine::new(daemon);
        let run = machine.run_event(
            "cursor",
            "PreToolUse",
            &cursor_shell(&machine, START),
            Some(PANE),
            CURSOR,
        );
        assert_eq!(
            cursor_answer(&run),
            json!({ "permission": "allow" }),
            "{daemon}"
        );
        assert!(
            run.elapsed < Duration::from_secs(6),
            "{daemon}: {:?}",
            run.elapsed
        );
        // The cause goes to the guard's log, never to the agent (B5).
        let log = machine.guard_log();
        match daemon {
            "unregistered" => assert_eq!(log, ""),
            "slow" => assert!(log.contains("\"cause\":\"deadline\""), "{log}"),
            _ => assert!(log.contains("daemon.unreachable"), "{daemon}: {log}"),
        }
    }
}

#[test]
fn a_cursor_permission_hook_whose_owner_handshake_fails_still_answers_allow() {
    let machine = Machine::new("registered");
    for event in ["PreToolUse", "SubagentStart"] {
        let mut child = std::process::Command::new(&machine.hook)
            .args(["hook", "--runtime", "cursor", "--event", event])
            .args(["--source", "hide-guidance@2"])
            .env_clear()
            .env("PATH", &machine.home)
            .env("HERDR_PANE_ID", PANE)
            .env("HIDE_PROCESS_OWNER_JOB", "no-such-job")
            .env(hide_platform::host::HOME_VARIABLE, &machine.home)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let _ = child
            .stdin
            .take()
            .unwrap()
            .write_all(cursor_shell(&machine, START).as_bytes());
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{event}: {:?}", output.status);
        let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(answer, json!({ "permission": "allow" }), "{event}");
    }
}

#[test]
fn inside_grok_the_claude_code_hook_and_cursors_entries_say_and_ask_nothing() {
    // Grok runs `~/.claude/settings.json` and `~/.cursor/hooks.json` beside
    // its own file; only its own hook may refuse (D-05).
    let machine = Machine::new("registered");
    let claude = json!({
        "session_id": "grok-session", "cwd": machine.checkout, "hook_event_name": "PreToolUse",
        "tool_name": "Bash", "tool_input": { "command": START },
    })
    .to_string();
    let run = machine.run_event("claude-code", "PreToolUse", &claude, Some(PANE), GROK);
    assert_eq!(run.stdout, "");
    for event in ["PreToolUse", "SubagentStart"] {
        let run = machine.run_event(
            "cursor",
            event,
            &cursor_shell(&machine, START),
            Some(PANE),
            GROK,
        );
        assert!(run.status_ok, "{event}");
        assert_eq!(run.stdout, "", "{event}");
    }
    assert!(
        machine.calls().is_empty(),
        "nothing but Grok's hook asked the daemon"
    );
    let run = machine.run_event(
        "grok",
        "PreToolUse",
        &grok_shell(&machine, START),
        Some(PANE),
        GROK,
    );
    assert!(machine.reason(&run).contains("hide agent spawn"));
    assert_eq!(machine.calls().len(), 1);
}

#[test]
fn a_grok_question_tool_of_the_current_factory_worker_is_redirected() {
    for tool in ["ask_user_question", "exit_plan_mode"] {
        let machine = Machine::new("worker");
        let payload = json!({
            "hook_event_name": "PreToolUse", "sessionId": "grok-session", "toolName": tool,
            "toolInput": { "questions": [{ "question": "private-question-text" }] },
        })
        .to_string();
        let run = machine.run_event("grok", "PreToolUse", &payload, Some(PANE), GROK);
        assert!(machine.reason(&run).contains("hide factory ask"), "{tool}");
        assert_eq!(
            machine.calls(),
            ["workspace factory-question-guard --session grok-session --runtime grok"]
        );
        assert!(!machine.guard_log().contains("private-question-text"));

        // Another Grok pane keeps its native question (B13).
        let machine = Machine::new("nonworker");
        let run = machine.run_event("grok", "PreToolUse", &payload, Some(PANE), GROK);
        assert!(run.status_ok);
        assert_eq!(run.stdout, "", "{tool}");
    }
}

/// A stand-in Herdr that answers every `pane.report_metadata` and keeps the
/// tokens it was told, in order.
struct Herdr {
    socket: PathBuf,
    reports: Arc<Mutex<Vec<Value>>>,
}

impl Herdr {
    fn start() -> Self {
        // A Unix socket path is capped at SUN_LEN, so it binds under /tmp.
        let dir = PathBuf::from("/tmp").join(format!(
            "hah-gc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("herdr.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let reports = Arc::new(Mutex::new(Vec::new()));
        let kept = reports.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut line = String::new();
                if BufReader::new(&stream).read_line(&mut line).is_err() {
                    continue;
                }
                let Ok(request) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                kept.lock()
                    .unwrap()
                    .push(request["params"]["tokens"].clone());
                let _ = writeln!(
                    stream,
                    "{}",
                    json!({"id": request["id"], "result": {"type": "ok"}})
                );
            }
        });
        Self { socket, reports }
    }

    /// `(working, done)` of every report so far.
    fn counts(&self) -> Vec<(String, String)> {
        self.reports
            .lock()
            .unwrap()
            .iter()
            .map(|tokens| {
                (
                    tokens["hide_sub_working"].as_str().unwrap().to_owned(),
                    tokens["hide_sub_done"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }
}

impl Drop for Herdr {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
        if let Some(dir) = self.socket.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

fn pair(working: u32, done: u32) -> (String, String) {
    (working.to_string(), done.to_string())
}

#[test]
fn grok_counts_its_subagents_and_keeps_background_ones_past_the_turn() {
    let machine = Machine::new("registered");
    let herdr = Herdr::start();
    let socket = herdr.socket.display().to_string();
    let env = |event: &'static str| {
        vec![
            ("GROK_HOOK_EVENT", event),
            ("GROK_SESSION_ID", "grok-session"),
            ("HERDR_SOCKET_PATH", socket.as_str()),
        ]
    };
    let grok = |event: &str, grok_event: &'static str, payload: &str| {
        let run = machine.run_event("grok", event, payload, Some(PANE), &env(grok_event));
        assert!(run.status_ok, "{event}");
        assert_eq!(run.stdout, "", "{event}: a passive event answers nothing");
    };
    grok(
        "SessionStart",
        "session_start",
        r#"{"sessionId":"grok-session"}"#,
    );
    grok(
        "SubagentStart",
        "subagent_start",
        r#"{"sessionId":"grok-session","subagentType":"explore"}"#,
    );
    grok(
        "SubagentStart",
        "subagent_start",
        r#"{"sessionId":"grok-session","subagentType":"general"}"#,
    );
    // The turn ends with one subagent still running in the background.
    grok(
        "Stop",
        "stop",
        r#"{"sessionId":"grok-session","reason":"end_turn","backgroundTasks":[
            {"id":"a","type":"subagent","status":"running","agentType":"general"},
            {"id":"b","type":"shell","status":"running","command":"npm run dev"}]}"#,
    );
    // A subagent's own stop leaves the pane's count alone.
    grok(
        "Stop",
        "stop",
        r#"{"sessionId":"child","subagentType":"general","backgroundTasks":[]}"#,
    );
    // The background subagent finishes in its own session.
    grok(
        "SubagentStop",
        "subagent_stop",
        r#"{"sessionId":"child","subagentType":"general"}"#,
    );
    assert_eq!(
        herdr.counts(),
        [pair(0, 0), pair(1, 0), pair(2, 0), pair(1, 0), pair(0, 1)]
    );
}

#[test]
fn cursor_counts_its_subagents_answers_allow_to_each_start_and_sweeps_at_the_turn_end() {
    let machine = Machine::new("registered");
    let herdr = Herdr::start();
    let socket = herdr.socket.display().to_string();
    let env = vec![
        ("CURSOR_VERSION", "2026.10.01"),
        ("HERDR_SOCKET_PATH", socket.as_str()),
    ];
    let start = json!({
        "conversation_id": "c1", "hook_event_name": "subagentStart", "subagent_id": "s1",
        "subagent_type": "explore", "task": "look", "parent_conversation_id": "c1",
        "tool_call_id": "t1", "is_parallel_worker": false,
    })
    .to_string();
    for _ in 0..2 {
        let run = machine.run_event("cursor", "SubagentStart", &start, Some(PANE), &env);
        assert_eq!(cursor_answer(&run), json!({ "permission": "allow" }));
    }
    let stop = json!({"conversation_id": "c1", "subagent_type": "explore", "status": "completed", "loop_count": 0}).to_string();
    let run = machine.run_event("cursor", "SubagentStop", &stop, Some(PANE), &env);
    assert_eq!(run.stdout, "", "subagentStop asks for no follow-up");
    let run = machine.run_event(
        "cursor",
        "Stop",
        r#"{"conversation_id":"c1","status":"completed","loop_count":0}"#,
        Some(PANE),
        &env,
    );
    assert_eq!(run.stdout, "", "stop asks for no follow-up");
    assert_eq!(
        herdr.counts(),
        [pair(1, 0), pair(2, 0), pair(1, 1), pair(0, 1)]
    );
}

#[test]
fn inside_grok_one_subagent_is_counted_once_however_many_of_hides_hooks_run() {
    // Grok runs Hide's Claude Code hook, Hide's Cursor entry and Hide's own
    // Grok file for the same subagent; only the last one counts (B4).
    let machine = Machine::new("registered");
    let herdr = Herdr::start();
    let socket = herdr.socket.display().to_string();
    let env = vec![
        ("GROK_HOOK_EVENT", "subagent_start"),
        ("GROK_SESSION_ID", "grok-session"),
        ("HERDR_SOCKET_PATH", socket.as_str()),
    ];
    let payload = r#"{"sessionId":"grok-session","subagentType":"explore"}"#;
    for runtime in ["claude-code", "cursor", "grok"] {
        let run = machine.run_event(runtime, "SubagentStart", payload, Some(PANE), &env);
        assert!(run.status_ok, "{runtime}");
        assert_eq!(run.stdout, "", "{runtime}");
    }
    assert_eq!(herdr.counts(), [pair(1, 0)]);
}
