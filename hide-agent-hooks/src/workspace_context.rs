//! A SessionStart capability probe through the same pane-scoped CLI the agent
//! will use. The hook never reads bearer bytes or trusts a pane ID on its own.

use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

const CLI_TIMEOUT: Duration = Duration::from_secs(2);
const OUTPUT_LIMIT: u64 = 16 * 1024;
const CAP_REF_ENV: &str = "HIDE_CAP_REF";

pub fn live_context() -> Option<String> {
    let program = cli_program()?;
    let inherited = std::env::var_os(CAP_REF_ENV).filter(|value| !value.is_empty());
    let (reference, created) = if let Some(value) = inherited {
        (PathBuf::from(value), false)
    } else {
        let answer = run_cli(&program, &["workspace", "bootstrap"], None)?;
        if answer["ok"] != true {
            return None;
        }
        (PathBuf::from(answer["reference"].as_str()?), true)
    };
    let result = run_cli(&program, &["workspace", "info"], Some(&reference))
        .filter(|answer| answer["ok"] == true)
        .and_then(|answer| format_context(&program, &reference, &answer));
    if result.is_none() && created {
        let _ = fs::remove_file(reference);
    }
    result
}

fn cli_program() -> Option<OsString> {
    let sibling = std::env::current_exe().ok()?.parent()?.join("hide");
    if sibling.is_file() {
        Some(sibling.into_os_string())
    } else {
        // An independently installed remote helper can use the CLI on PATH.
        Some(OsString::from("hide"))
    }
}

fn run_cli(program: &OsString, arguments: &[&str], reference: Option<&Path>) -> Option<Value> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(reference) = reference {
        command.env(CAP_REF_ENV, reference);
    }
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + CLI_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        return None;
    }
    let mut output = Vec::new();
    child
        .stdout
        .take()?
        .take(OUTPUT_LIMIT)
        .read_to_end(&mut output)
        .ok()?;
    serde_json::from_slice(&output).ok()
}

fn format_context(program: &OsString, reference: &Path, answer: &Value) -> Option<String> {
    let result = answer.get("result")?;
    let identity = result.get("context")?;
    identity.get("device_id")?.as_str()?;
    identity.get("workspace_id")?.as_str()?;
    let capabilities = result.get("capabilities")?.as_array()?;
    let has = |name: &str| capabilities.iter().any(|item| item.as_str() == Some(name));
    if !has("workspace.info") {
        return None;
    }
    let mut commands = vec!["workspace info"];
    if has("file.open") {
        commands.push("file open <path> [--beside] [--reveal]");
    }
    if has("diff.open") {
        commands.push("diff open <path> [--beside] [--reveal]");
    }
    if has("browser.open") {
        commands.push("browser open <url-or-path> [--reveal] [--wait]");
    }
    if has("view.list") {
        commands.push("view list");
    }
    if has("browser.status") {
        commands.push("view status <view-id>");
    }
    if has("view.select") {
        commands.push("view select <view-id> [--reveal]");
    }
    if has("view.split") {
        commands.push("view split <view-id> --area <area-id> --edge left|right|up|down");
    }
    if has("view.move") {
        commands.push("view move <view-id> --area <area-id> --index <n>");
    }
    if has("view.close") {
        commands.push("view close <view-id>");
    }
    let command_prefix = format!(
        "HIDE_CAP_REF={} {}",
        shell_quote(&reference.to_string_lossy()),
        shell_quote(&program.to_string_lossy()),
    );
    Some(format!(
        "Hide Workspace control is available for this connected pane. Commands affect only this pane's Workspace; there is no Workspace override. Prefix each command with `{command_prefix}`. Available commands: {}. Run `{} workspace info` to refresh capabilities and `{} --help` for syntax. Omit `--reveal` to leave the current screen and keyboard focus unchanged. A failed command returns a reason and next action; recheck before retrying a timed-out action.",
        commands.join(", "),
        command_prefix,
        shell_quote(&program.to_string_lossy()),
    ))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
