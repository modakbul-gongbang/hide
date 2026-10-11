//! OpenCode's plugin and Pi's and omp's extension on Windows
//! (`docs/agent-hooks.md`, Installing): the file this build writes, loaded in
//! Node by `tests/windows/scripts.mjs` as each host loads it, starts the real
//! helper by the path the file names, with no shell, from a folder whose name
//! has a space, a quote, brackets and a `$`. The session's guidance reaching
//! its prompt is the proof that the spawn, stdin and stdout crossed Windows.
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

/// The texts the script added to its session's prompts.
fn prompts(agent: &str, file: &PluginFile, home: &Path, helper: &Path) -> Vec<String> {
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
        ("opencode", &opencode::PLUGIN),
        ("pi", &pi_extension::PI),
        ("omp", &pi_extension::OMP),
    ] {
        let added = prompts(agent, file, home, &helper);
        assert!(
            added.iter().any(|text| text.contains(GUIDANCE_LINE)),
            "{agent}: no prompt carried the guidance: {added:?}"
        );
    }
}
