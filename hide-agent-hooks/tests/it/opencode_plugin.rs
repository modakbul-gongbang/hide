//! Hide's OpenCode plugin as this build writes it, run in Node by
//! `tests/opencode/plugin.test.mjs` against a stand-in helper (PRD
//! opencode-plugin D-13), and its bytes pinned like the other hooks'.
//!
//! Node is required, not optional: the plugin is JavaScript and a run without
//! Node proves nothing about it, so a missing `node` fails here.

use std::path::{Path, PathBuf};
use std::process::Command;

use hide_agent_hooks::opencode::plugin_text;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn the_plugin_keeps_its_pinned_bytes() {
    assert_eq!(
        plugin_text(Path::new("/kit path/it's/hide-agent-hooks")),
        include_str!("../fixtures/hook-bytes/opencode-plugin.js"),
        "the plugin changed: raise PLUGIN_VERSION and regenerate the fixture"
    );
}

#[cfg(unix)]
#[test]
fn the_plugin_behaves_as_opencode_runs_it() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let helper = dir.path().join("hide-agent-hooks");
    let stand_in = crate_dir().join("tests/opencode/helper.mjs");
    std::fs::write(
        &helper,
        format!("#!/bin/sh\nexec node '{}' \"$@\"\n", stand_in.display()),
    )
    .unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    // `.mjs`: Node reads the plugin as the ES module OpenCode loads.
    let plugin = dir.path().join("hide.mjs");
    std::fs::write(&plugin, plugin_text(&helper)).unwrap();

    let output = Command::new("node")
        .args(["--test", "--test-timeout=60000"])
        .arg(crate_dir().join("tests/opencode/plugin.test.mjs"))
        .env("HIDE_OPENCODE_PLUGIN", &plugin)
        .env("HIDE_OPENCODE_HELPER_LOG", dir.path().join("calls.jsonl"))
        .env(
            "HIDE_OPENCODE_HELPER_ANSWERS",
            dir.path().join("answers.json"),
        )
        .env_remove("HERDR_ENV")
        .env_remove("HERDR_PANE_ID")
        .env_remove("HERDR_SOCKET_PATH")
        .output()
        .expect("node runs Hide's OpenCode plugin test; install Node 22 or later");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
