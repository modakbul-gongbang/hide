//! OpenCode's plugin and Pi's and omp's extension on Windows
//! (`docs/agent-hooks.md`, Installing): the file this build writes, loaded in
//! Node by `tests/windows/scripts.mjs` as each host loads it, starts the real
//! helper by the path the file names, with no shell, from a folder whose name
//! has a space, a quote, brackets and a `$`. The session's guidance reaching
//! its prompt is the proof that the spawn, stdin and stdout crossed Windows,
//! and a tool result's ask for the letters due while the session works
//! (`letters`, PRD letters-to-busy-agents) is proved the same way: the
//! helper it started reached for the `hide` beside it, which this folder
//! does not hold, and recorded that in the home's delivery diagnostics,
//! while the result went on unchanged. No daemon runs on the runner, so a
//! letter reaching the result is proved by the Unix script tests
//! (`opencode_plugin.rs`, `pi_extension.rs`), which `docs/INSTALL.md`
//! records.
//!
//! Node is required, not optional: a run without it proves nothing about the
//! scripts, so a missing `node` fails here.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;

use hide_agent_hooks::guidance::GUIDANCE_LINE;
use hide_agent_hooks::plugin::PluginFile;
use hide_agent_hooks::{HELPER_BINARY_NAME, opencode, pi_extension};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// What the script answered: the texts it added to its session's prompts
/// and the tool result it left.
#[derive(serde::Deserialize)]
struct Answered {
    prompts: Vec<String>,
    tool_result: String,
}

fn run(agent: &str, file: &PluginFile, home: &Path, helper: &Path) -> Answered {
    // `.mjs`: Node reads the file as the ES module each host loads.
    let script = home.join(format!("{agent}.mjs"));
    std::fs::write(&script, file.text(helper)).unwrap();
    let output = Command::new("node")
        .arg(crate_dir().join("tests/windows/scripts.mjs"))
        .current_dir(home)
        .env("HIDE_SCRIPT", &script)
        .env("HIDE_SCRIPT_AGENT", agent)
        .env(hide_platform::host::HOME_VARIABLE, home)
        .env_remove("HIDE_CAP_REF")
        .output()
        .expect("node is required to run the agents' scripts");
    assert!(
        output.status.success(),
        "{agent}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{agent}: {error}: {:?}", output))
}

#[test]
fn the_scripts_start_the_helper_by_its_path_and_carry_its_guidance() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    // `[`, `]` and `$` are what a shell could take for a wildcard or a
    // variable; the script starts the helper with none.
    let helper = home.join("it's [1] $x here").join(HELPER_BINARY_NAME);
    std::fs::create_dir_all(helper.parent().unwrap()).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_hide-agent-hooks"), &helper).unwrap();

    for (agent, file) in [
        ("opencode", &opencode::V1),
        ("pi", &pi_extension::PI),
        ("omp", &pi_extension::OMP),
    ] {
        let answered = run(agent, file, home, &helper);
        let added = &answered.prompts;
        assert!(
            added.iter().any(|text| text.contains(GUIDANCE_LINE)),
            "{agent}: no prompt carried the guidance: {added:?}"
        );
        assert_eq!(answered.tool_result, "RESULT", "{agent}");
        // The ask ran the helper, which started no `hide` because none is
        // beside it, and said so once in the home's delivery diagnostics.
        let diagnostics = std::fs::read_to_string(
            home.join(".hide")
                .join("agent-hooks")
                .join("delivery-diagnostics.json"),
        )
        .unwrap_or_else(|error| panic!("{agent}: no delivery diagnostics: {error}"));
        assert!(diagnostics.contains("\"cli\""), "{agent}: {diagnostics}");
    }
}
