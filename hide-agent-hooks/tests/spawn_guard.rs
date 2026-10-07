//! The spawn guard (PRD herdr-spawn-guard B1, B2, B4 to B7, B11 to B13), run as
//! an agent runs it: the compiled helper with a runtime's `PreToolUse` payload
//! on stdin, beside a stand-in `hide` that answers `workspace bootstrap` the way
//! the daemon does (registered, refused, not running, or not answering) and
//! records every call. What it proves is what the agent observes: the call is
//! refused with the filled `hide agent spawn` command, or nothing is printed and
//! the call runs.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use hide_platform::process::OwnedChild;

const PANE: &str = "w1:p2";

/// A freshly copied executable pays a first-exec validation cost on macOS that
/// would eat the guard's own budget, so each is run once under its own bound
/// before the guard is measured against it.
fn warm_first_exec(program: &Path) {
    let mut command = Command::new(program);
    command
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = OwnedChild::spawn(&mut command).unwrap();
    child
        .capture_until(Instant::now() + Duration::from_secs(5), 1)
        .expect("first-exec warming must finish within its own bound");
}

struct Machine {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    hook: PathBuf,
    /// A checkout on branch `topic`, the agent's working directory.
    checkout: PathBuf,
}

struct Run {
    stdout: String,
    stderr: String,
    status_ok: bool,
    elapsed: Duration,
}

impl Machine {
    fn new(daemon: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let home = root.join("home");
        let bin = root.join("bin");
        let checkout = root.join("repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(checkout.join(".git")).unwrap();
        std::fs::write(checkout.join(".git/HEAD"), "ref: refs/heads/topic\n").unwrap();
        let hook = bin.join("hide-agent-hooks");
        std::fs::copy(env!("CARGO_BIN_EXE_hide-agent-hooks"), &hook).unwrap();
        let fake = bin.join("hide");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\n[ \"$1\" = --help ] && exit 0\necho \"$@\" >> '{calls}'\ncase \"$(/bin/cat '{mode}')\" in\n  registered) echo '{{\"ok\":true,\"reference\":\"/x\"}}' ;;\n  unregistered) echo checkout_not_registered >&2; exit 2 ;;\n  down) echo hide_unavailable >&2; exit 2 ;;\n  mute) exit 2 ;;\n  bridge) echo bridge_unavailable >&2; exit 2 ;;\n  later) echo a_reason_a_later_build_adds >&2; exit 2 ;;\n  slow) /bin/sleep 30 ;;\nesac\n",
                calls = root.join("hide-calls").display(),
                mode = root.join("mode").display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(root.join("mode"), daemon).unwrap();
        warm_first_exec(&hook);
        warm_first_exec(&fake);
        std::fs::remove_file(root.join("hide-calls")).ok();
        Self {
            _dir: dir,
            root,
            home,
            hook,
            checkout,
        }
    }

    fn payload(&self, command: &str) -> String {
        serde_json::json!({
            "session_id": "s1",
            "cwd": self.checkout,
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": command },
        })
        .to_string()
    }

    /// One guard run as `runtime` runs its entry for `payload`, with `pane` as
    /// its pane and `extra` in its environment.
    fn run(&self, runtime: &str, payload: &str, pane: Option<&str>, extra: &[(&str, &str)]) -> Run {
        let mut command = Command::new(&self.hook);
        command
            .args(["hook", "--runtime", runtime, "--event", "PreToolUse"])
            .args(["--source", "hide-subagents@6"])
            .env(hide_platform::host::HOME_VARIABLE, &self.home)
            .env("PATH", &self.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy();
            if ["HERDR_", "HIDE_", "HCOORD_", "CURSOR_", "OPENCODE", "GROK_"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
            {
                command.env_remove(key);
            }
        }
        if let Some(pane) = pane {
            command.env("HERDR_PANE_ID", pane);
        }
        command.envs(extra.iter().copied());
        let started = Instant::now();
        let mut child = OwnedChild::spawn(&mut command).unwrap();
        let mut stdin = child.take_stdin().unwrap();
        stdin.write_all(payload.as_bytes()).unwrap();
        drop(stdin);
        let output = child
            .capture_until(Instant::now() + Duration::from_secs(15), 64 * 1024)
            .expect("the guard ends within its bound");
        Run {
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
            status_ok: output.status.success(),
            elapsed: started.elapsed(),
        }
    }

    /// The calls the guard made of `hide`.
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("hide-calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn guard_log(&self) -> String {
        std::fs::read_to_string(self.home.join(".hide/agent-hooks/spawn-guard.log"))
            .unwrap_or_default()
    }

    fn reason(&self, run: &Run) -> String {
        let answer: serde_json::Value = serde_json::from_str(run.stdout.trim()).expect(&run.stdout);
        assert_eq!(answer["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(answer["hookSpecificOutput"]["permissionDecision"], "deny");
        answer["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

const START: &str = "herdr agent start set-g --kind claude --pane w1:p9 -- --model opus";

#[test]
fn a_launch_in_a_registered_checkout_is_refused_with_the_command_to_use_instead() {
    for runtime in ["claude-code", "codex"] {
        let machine = Machine::new("registered");
        let run = machine.run(runtime, &machine.payload(START), Some(PANE), &[]);

        assert!(run.status_ok, "a refusal is an answer, not a failure");
        let reason = machine.reason(&run);
        // B1: the name, kind, repository and current branch filled, the intent left.
        let repo = machine.checkout.display();
        assert!(
            reason.contains(&format!(
                "hide agent spawn --parent here --name set-g --intent <intent> --kind claude --repo {repo} --branch topic -- --model opus"
            )),
            "{runtime}: {reason}"
        );
        assert!(reason.contains(&format!(
            "hide agent spawn --name set-g --intent <intent> --kind claude --repo {repo} --branch topic -- --model opus"
        )), "{runtime}: {reason}");
        assert!(reason.contains("root agent, no automatic watch"));
        assert_eq!(machine.calls(), ["workspace bootstrap"], "{runtime}");
        // B12: the pane, the kind and the shape are logged, the command is not.
        let log = machine.guard_log();
        assert!(
            log.contains(PANE)
                && log.contains("\"agent\":\"claude\"")
                && log.contains("agent_start"),
            "{log}"
        );
        assert!(!log.contains("set-g") && !log.contains("--model"), "{log}");
    }
}

#[test]
fn a_pane_run_or_send_text_launch_leaves_the_name_and_intent_for_the_agent() {
    let machine = Machine::new("registered");
    for command in [
        "herdr pane run w1:p9 codex --model gpt-6 'fix it'",
        "cd /tmp && herdr pane send-text w1:p9 'codex --model gpt-6 \"fix it\"'",
    ] {
        let run = machine.run("codex", &machine.payload(command), Some(PANE), &[]);
        let reason = machine.reason(&run);
        assert!(
            reason.contains("--name <name> --intent <intent> --kind codex"),
            "{command}: {reason}"
        );
        assert!(
            reason.contains(" --branch topic -- --model gpt-6"),
            "{reason}"
        );
    }
}

#[test]
fn what_is_not_a_launch_runs_and_never_asks_the_daemon() {
    let machine = Machine::new("registered");
    for command in [
        "cargo test -p hide-agent-hooks",
        "herdr pane split w1:p9 --direction right",
        "herdr tab create --workspace w1",
        "herdr pane run w1:p9 'npm run dev'",
        "git commit -m 'herdr agent start a --kind claude --pane p'",
    ] {
        let run = machine.run("claude-code", &machine.payload(command), Some(PANE), &[]);
        assert!(run.status_ok);
        assert_eq!(run.stdout, "", "{command}");
    }
    // B13: not even a process for `hide` was started for any of them.
    assert!(machine.calls().is_empty(), "{:?}", machine.calls());
    assert_eq!(machine.guard_log(), "");
}

#[test]
fn outside_a_registered_checkout_or_a_herdr_pane_the_same_command_runs() {
    let machine = Machine::new("unregistered");
    let refused = machine.run("claude-code", &machine.payload(START), Some(PANE), &[]);
    assert!(refused.status_ok);
    assert_eq!(
        refused.stdout, "",
        "B5: the daemon does not know this checkout"
    );
    assert_eq!(
        refused.stderr, "",
        "a refusal by the daemon is not a diagnostic"
    );
    assert_eq!(machine.calls(), ["workspace bootstrap"]);

    let machine = Machine::new("registered");
    let plain = machine.run("claude-code", &machine.payload(START), None, &[]);
    assert!(plain.status_ok);
    assert_eq!(plain.stdout, "", "B5: a plain terminal has no pane");
    assert!(machine.calls().is_empty());
}

#[test]
fn a_daemon_that_cannot_be_asked_lets_the_call_run_and_leaves_one_diagnostic() {
    // A daemon that is down, mute, a device bridge that is gone and a reason this
    // build does not know are all "could not ask", never "not Hide's".
    for daemon in ["down", "mute", "bridge", "later"] {
        let machine = Machine::new(daemon);
        let first = machine.run("claude-code", &machine.payload(START), Some(PANE), &[]);
        assert!(first.status_ok, "B6: the hook does not fail the call");
        assert_eq!(first.stdout, "", "nothing on screen");
        assert_eq!(first.stderr, "", "nothing on the hook's stderr either");
        // The diagnostic is one line of the guard's log, and a repeat within the
        // throttle window adds none.
        let second = machine.run("codex", &machine.payload(START), Some(PANE), &[]);
        assert_eq!(second.stdout, "");
        assert_eq!(second.stderr, "");
        let log = machine.guard_log();
        assert_eq!(log.lines().count(), 1, "{daemon}: {log}");
        let line: serde_json::Value = serde_json::from_str(log.trim()).unwrap();
        assert_eq!(line["kind"], "daemon.unreachable", "{daemon}");
        assert_eq!(line["runtime"], "claude-code");
        assert_eq!(line["cause"], "daemon");
    }
}

#[test]
fn a_daemon_that_does_not_answer_in_time_lets_the_call_run() {
    let machine = Machine::new("slow");
    let run = machine.run("claude-code", &machine.payload(START), Some(PANE), &[]);
    assert!(run.status_ok);
    assert_eq!(run.stdout, "");
    assert!(
        machine.guard_log().contains("\"cause\":\"deadline\""),
        "{}",
        machine.guard_log()
    );
    assert!(
        run.elapsed < Duration::from_secs(6),
        "the guard stays inside the entry's own timeout: {:?}",
        run.elapsed
    );
}

#[test]
fn inside_another_agents_session_the_guard_stays_out() {
    for origin in [
        &[("GROK_SESSION_ID", "s1")][..],
        &[("OPENCODE", "1")][..],
        &[("CURSOR_VERSION", "1")][..],
    ] {
        let machine = Machine::new("registered");
        let run = machine.run("claude-code", &machine.payload(START), Some(PANE), origin);
        assert!(run.status_ok);
        assert_eq!(run.stdout, "", "{origin:?}");
        assert!(machine.calls().is_empty(), "{origin:?}");
    }
}

#[test]
fn a_payload_the_guard_cannot_read_lets_the_call_run() {
    let machine = Machine::new("registered");
    for payload in [
        "herdr agent start but not json",
        &serde_json::json!({"tool_name": "Edit", "tool_input": {"command": START}}).to_string(),
        &serde_json::json!({"tool_name": "Bash", "tool_input": {"command": ["herdr", "agent", "start"]}}).to_string(),
        "",
    ] {
        let run = machine.run("claude-code", payload, Some(PANE), &[]);
        assert!(run.status_ok, "{payload}");
        assert_eq!(run.stdout, "", "{payload}");
    }
    assert!(machine.calls().is_empty());
}

#[test]
fn a_checkout_with_a_detached_head_leaves_the_branch_for_the_agent() {
    let machine = Machine::new("registered");
    std::fs::write(
        machine.checkout.join(".git/HEAD"),
        "0123456789abcdef0123456789abcdef01234567\n",
    )
    .unwrap();
    let run = machine.run("claude-code", &machine.payload(START), Some(PANE), &[]);
    assert!(machine.reason(&run).contains("--branch <branch>"));
}

#[test]
fn a_failed_owner_handshake_never_ends_the_guard_with_a_refusal_code() {
    // Exit 2 from a pre-tool hook blocks the call; the guard's own bookkeeping
    // failing must not do that.
    let machine = Machine::new("registered");
    // Another system's launch metadata is the handshake's refusal on Unix. A
    // plain `Command`, because the owned launcher clears these variables.
    let mut child = Command::new(&machine.hook)
        .args(["hook", "--runtime", "claude-code", "--event", "PreToolUse"])
        .args(["--source", "hide-subagents@6"])
        .env_clear()
        .env("PATH", &machine.home)
        .env("HERDR_PANE_ID", PANE)
        .env("HIDE_PROCESS_OWNER_JOB", "no-such-job")
        .env(hide_platform::host::HOME_VARIABLE, &machine.home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(machine.payload(START).as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output.status);
    assert_eq!(output.stdout, b"");
}

#[test]
fn a_call_aimed_at_another_herdr_runs_in_a_registered_checkout() {
    let machine = Machine::new("registered");
    let run = machine.run(
        "claude-code",
        &machine
            .payload("HERDR_SOCKET_PATH=/tmp/qa.sock herdr agent start qa --kind claude --pane p"),
        Some(PANE),
        &[("HERDR_SOCKET_PATH", "/run/own.sock")],
    );
    assert!(run.status_ok);
    assert_eq!(run.stdout, "");
    assert!(machine.calls().is_empty(), "{:?}", machine.calls());
}
