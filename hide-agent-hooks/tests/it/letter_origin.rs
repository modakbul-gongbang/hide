//! Claude Code's hook inside another agent's session (PRD settings-cleanup
//! D-25): run as the agent runs it, the compiled helper beside a stand-in
//! `hide` that answers `inbox` with one letter and records every call.
//! Outside such a session the hook takes and confirms the letter; inside
//! Grok or OpenCode it takes and confirms nothing, and its counters, Memory
//! and guidance are what they were.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use hide_platform::process::OwnedChild;

const PANE: &str = "w1:p1";

/// A freshly copied executable pays a first-exec validation cost on macOS
/// that would eat the hook's 1.65 second intake budget, so each is run once
/// under its own bound before the hook is measured against it.
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

/// The helper copied beside a fake `hide`, since the hook asks its sibling.
struct Machine {
    _dir: tempfile::TempDir,
    home: PathBuf,
    hook: PathBuf,
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
        let hook = bin.join("hide-agent-hooks");
        std::fs::copy(env!("CARGO_BIN_EXE_hide-agent-hooks"), &hook).unwrap();
        let calls = root.join("hide-calls");
        let fake = bin.join("hide");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\necho \"$@\" >> '{}'\ncase \"$2\" in\n  --hook) echo '{{\"ok\":true,\"result\":{{\"context\":\"LETTER-FOR-THIS-PANE\",\"ids\":[\"letter-1\"]}}}}' ;;\n  --confirm) echo '{{\"ok\":true,\"result\":{{\"confirmed\":[\"letter-1\"]}}}}' ;;\n  *) exit 1 ;;\nesac\n",
                calls.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        warm_first_exec(&hook);
        warm_first_exec(&fake);
        std::fs::remove_file(&calls).ok();
        Self {
            _dir: dir,
            home,
            hook,
            calls,
        }
    }

    /// One hook run of Claude Code's entry, with `origin` in its environment.
    fn run(&self, event: &str, origin: &[(&str, &str)]) -> String {
        self.run_as("claude-code", event, origin)
    }

    fn run_as(&self, runtime: &str, event: &str, origin: &[(&str, &str)]) -> String {
        let mut command = Command::new(&self.hook);
        command
            .args(["hook", "--runtime", runtime, "--event", event])
            .args(["--source", "hide-subagents@6"])
            .env(hide_platform::host::HOME_VARIABLE, &self.home)
            .env("PATH", &self.home)
            .env("HERDR_PANE_ID", PANE)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy();
            if ["HERDR_", "HIDE_", "HCOORD_", "CURSOR_", "OPENCODE", "GROK_"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
                && name != "HERDR_PANE_ID"
            {
                command.env_remove(key);
            }
        }
        command.envs(origin.iter().copied());
        let mut child = OwnedChild::spawn(&mut command).unwrap();
        let output = child
            .capture_until(Instant::now() + Duration::from_secs(15), 64 * 1024)
            .expect("the hook ends within its bound");
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    }

    /// The `hide inbox` calls the hook made; the guidance asks `hide` other
    /// things (the Workspace, which the stand-in does not answer).
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.calls)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.starts_with("inbox"))
            .map(str::to_owned)
            .collect()
    }

    /// Every file Hide's hook left under the home, with its bytes.
    fn files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, found: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, found);
                } else {
                    found.insert(path.clone(), std::fs::read(&path).unwrap_or_default());
                }
            }
        }
        let mut found = BTreeMap::new();
        walk(&self.home, &mut found);
        found
    }
}

#[test]
fn outside_another_agent_the_hook_takes_and_confirms_the_letter() {
    let machine = Machine::new();

    let stdout = machine.run("UserPromptSubmit", &[]);

    assert!(stdout.contains("LETTER-FOR-THIS-PANE"), "{stdout}");
    assert_eq!(
        machine.calls(),
        ["inbox --hook", "inbox --confirm letter-1"]
    );
}

#[test]
fn inside_grok_or_opencode_the_hook_takes_and_confirms_no_letter() {
    for origin in [
        &[("OPENCODE", "1"), ("OPENCODE_PID", "4242")][..],
        &[
            ("GROK_HOOK_EVENT", "UserPromptSubmit"),
            ("GROK_SESSION_ID", "s1"),
        ][..],
    ] {
        let machine = Machine::new();

        let stdout = machine.run("UserPromptSubmit", origin);

        assert_eq!(stdout, "", "{origin:?}");
        assert!(
            machine.calls().is_empty(),
            "{origin:?}: {:?}",
            machine.calls()
        );
    }
}

#[test]
fn inside_grok_or_opencode_guidance_and_counters_are_what_they_are_outside() {
    let outside = Machine::new();
    let guidance = outside.run("SessionStart", &[]);
    outside.run("SubagentStart", &[]);
    assert!(guidance.contains("hookSpecificOutput"), "{guidance}");
    assert!(!outside.files().is_empty(), "the counters wrote a record");

    for origin in [
        &[("OPENCODE", "1")][..],
        &[("GROK_HOOK_EVENT", "SessionStart")][..],
    ] {
        let inside = Machine::new();

        let stdout = inside.run("SessionStart", origin);
        inside.run("SubagentStart", origin);

        assert_eq!(stdout, guidance, "{origin:?}");
        let names = |machine: &Machine| {
            machine
                .files()
                .into_keys()
                .map(|path| path.strip_prefix(&machine.home).unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&inside), names(&outside), "{origin:?}");
        assert!(inside.calls().is_empty(), "{origin:?}");
    }
}

#[test]
fn cursor_still_silences_the_whole_hook() {
    let machine = Machine::new();

    assert_eq!(
        machine.run("SessionStart", &[("CURSOR_VERSION", "2.0.0")]),
        ""
    );
    assert_eq!(
        machine.run("UserPromptSubmit", &[("CURSOR_VERSION", "2.0.0")]),
        ""
    );
    assert!(machine.calls().is_empty());
    assert!(machine.files().is_empty());
}

#[test]
fn cursor_silences_claude_aliases_before_any_hook_effect() {
    let machine = Machine::new();
    for runtime in ["claude", " CLAUDE_CODE ", " CLAUDE-CODE "] {
        for event in ["SessionStart", "UserPromptSubmit", "PreToolUse", "Stop"] {
            assert_eq!(
                machine.run_as(runtime, event, &[("CURSOR_VERSION", "2.0.0")]),
                "",
                "{runtime}: {event}"
            );
        }
    }
    assert!(machine.calls().is_empty());
    assert!(machine.files().is_empty());
}
