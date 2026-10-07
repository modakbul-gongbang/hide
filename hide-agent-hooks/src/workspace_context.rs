//! A SessionStart capability probe through the same scoped CLI the agent will
//! use. The hook never reads bearer bytes or trusts a pane ID on its own, and
//! it needs no pane id at all: the daemon binds a caller in a pane to that
//! pane and any other local caller to the registered checkout holding its cwd,
//! so the guidance names the checkout the daemon answered with.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use hide_platform::process::OwnedChild;
use serde_json::Value;

const CLI_TIMEOUT: Duration = Duration::from_secs(2);
const OUTPUT_LIMIT: u64 = 16 * 1024;
const CAP_REF_ENV: &str = "HIDE_CAP_REF";

pub fn live_context() -> Option<String> {
    let program = cli_program()?;
    // The daemon removes a reference's file when it expires or is revoked;
    // a command naming that path still runs on a bare bootstrap, but the
    // context would hand the next session a reference that is gone.
    let inherited = std::env::var_os(CAP_REF_ENV)
        .filter(|value| !value.is_empty() && Path::new(value).is_file());
    let reference = if let Some(value) = inherited {
        PathBuf::from(value)
    } else {
        let answer = run_cli(&program, &["workspace", "bootstrap"], None)?;
        if answer["ok"] != true {
            return None;
        }
        PathBuf::from(answer["reference"].as_str()?)
    };
    run_cli(&program, &["workspace", "info"], Some(&reference))
        .filter(|answer| answer["ok"] == true)
        .and_then(|answer| format_context(&program, &reference, &answer))
}

pub fn cli_program() -> Option<OsString> {
    let sibling = std::env::current_exe()
        .ok()?
        .parent()?
        .join(format!("hide{}", std::env::consts::EXE_SUFFIX));
    if sibling.is_file() {
        Some(sibling.into_os_string())
    } else {
        // An independently installed remote helper can use the CLI on PATH.
        Some(OsString::from("hide"))
    }
}

#[allow(clippy::disallowed_methods)] // a production wait, not test code
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
    let mut child = OwnedChild::spawn(&mut command).ok()?;
    let deadline = Instant::now() + CLI_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill_tree();
                let _ = child.wait();
                return None;
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill_tree();
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
        .take_stdout()?
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
    let checkout_path = identity.get("checkout_path")?.as_str()?;
    let capabilities = result.get("capabilities")?.as_array()?;
    let has = |name: &str| capabilities.iter().any(|item| item.as_str() == Some(name));
    if !has("workspace.info") {
        return None;
    }
    let mut commands = vec!["workspace info"];
    if has("request.send") {
        commands.push(
            "request send <target> --intent <key> --body <text> [--kind request|block|report]",
        );
    }
    if has("inbox") {
        commands.push("inbox");
    }
    if has("watch.assign") {
        commands
            .push("watch assign <watch-or-target-id> --observer <id> [--expected-generation <n>]");
    }
    if has("file.open") {
        commands.push("file open <path> [--beside] [--reveal]");
    }
    if has("diff.open") {
        commands.push("diff open <path> [--beside] [--reveal]");
    }
    if has("browser.open") {
        commands.push("browser open <url-or-path> [--reveal] [--wait]");
        commands.push(
            "browser snapshot|click|fill|type|press|hover|drag|scroll|wait|screenshot|eval|console|network <display> ... (read and drive a browser display; `browser help` explains refs, checking with `--diff`, and failures)",
        );
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
        "Hide Workspace control is available for this session's checkout `{checkout_path}`. Workspace commands affect only that checkout; there is no Workspace override. Delivery commands use the current agent pane and native session, including on a connected device. Prefix each command with `{command_prefix}`. Available commands: {}. Run `{} workspace info` to refresh capabilities and `{} --help` for syntax. Omit `--reveal` to leave the current screen and keyboard focus unchanged. A failed command returns a reason and next action; recheck before retrying a timed-out action.",
        commands.join(", "),
        command_prefix,
        shell_quote(&program.to_string_lossy()),
    ))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn guidance_names_the_checkout_the_daemon_bound_the_caller_to() {
        let answer = json!({
            "ok": true,
            "result": {
                "context": {
                    "device_id": "local",
                    "workspace_id": "workspace:1",
                    "checkout_id": "workspace:1:checkout:2",
                    "checkout_path": "/srv/project"
                },
                "capabilities": ["workspace.info", "browser.open", "view.close"]
            }
        });
        let context = format_context(
            &OsString::from("/opt/hide/hide"),
            Path::new("/srv/state/pane-capabilities/ref.json"),
            &answer,
        )
        .unwrap();
        assert!(context.starts_with(
            "Hide Workspace control is available for this session's checkout `/srv/project`."
        ));
        assert!(
            context
                .contains("HIDE_CAP_REF='/srv/state/pane-capabilities/ref.json' '/opt/hide/hide'")
        );
        assert!(context.contains("browser open <url-or-path> [--reveal] [--wait], browser snapshot|click|fill|type|press|hover|drag|scroll|wait|screenshot|eval|console|network <display> ... (read and drive a browser display; `browser help` explains refs, checking with `--diff`, and failures), view close <view-id>"));
        assert!(!context.contains("file open"));
    }

    #[test]
    fn an_answer_without_a_checkout_or_workspace_info_yields_no_guidance() {
        let no_checkout = json!({"ok": true, "result": {"context": {"device_id": "local", "workspace_id": "w"}, "capabilities": ["workspace.info"]}});
        assert!(format_context(&OsString::from("hide"), Path::new("/r"), &no_checkout).is_none());
        let no_info = json!({"ok": true, "result": {"context": {"device_id": "local", "workspace_id": "w", "checkout_path": "/srv/p"}, "capabilities": ["view.list"]}});
        assert!(format_context(&OsString::from("hide"), Path::new("/r"), &no_info).is_none());
    }
}
