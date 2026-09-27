use std::io;
use std::process::Command;
use std::time::Duration;

use herdr_core::workspace_control::{Action, Edge};

use crate::env::{self, Env};
use crate::spawn::spawn_owned;
use crate::state_file::{self, DaemonState};

#[derive(Debug, Eq, PartialEq)]
pub enum CommandKind {
    Help,
    Open,
    /// `hide connect`: `open` without the browser, answered as one JSON line
    /// for a host that loads the shell itself (the desktop app).
    Connect,
    Status {
        json: bool,
    },
    Stop,
    Serve {
        keep_alive: bool,
    },
    Dev,
    BrowserOpen {
        target: String,
        reveal: bool,
        wait: bool,
        request_id: Option<String>,
    },
    WorkspaceBootstrap,
    WorkspaceInfo,
    ViewList,
    ViewStatus {
        view_id: String,
    },
    WorkspaceAction {
        action: Action,
        request_id: Option<String>,
    },
}

const BROWSER_USAGE: &str =
    "usage: hide browser open <url-or-path> [--reveal] [--wait] [--request-id <id>]";

pub fn parse_args(args: &[String]) -> Result<CommandKind, String> {
    let mut iter = args.iter().skip(1);
    match iter.next().map(String::as_str) {
        Some("help" | "--help" | "-h") if iter.next().is_none() => Ok(CommandKind::Help),
        None | Some("open") => Ok(CommandKind::Open),
        Some("connect") => Ok(CommandKind::Connect),
        Some("status") => {
            let json = iter.any(|arg| arg == "--json");
            Ok(CommandKind::Status { json })
        }
        Some("stop") => Ok(CommandKind::Stop),
        Some("serve") => {
            let keep_alive = iter.any(|arg| arg == "--keep-alive");
            Ok(CommandKind::Serve { keep_alive })
        }
        Some("dev") => Ok(CommandKind::Dev),
        Some("browser") => parse_browser(iter),
        Some("workspace") => match (iter.next().map(String::as_str), iter.next()) {
            (Some("bootstrap"), None) => Ok(CommandKind::WorkspaceBootstrap),
            (Some("info"), None) => Ok(CommandKind::WorkspaceInfo),
            _ => Err("usage: hide workspace info".to_owned()),
        },
        Some("file") => parse_open(iter, true),
        Some("diff") => parse_open(iter, false),
        Some("view") => parse_view(iter),
        Some(other) => Err(format!("unknown command: {other}")),
    }
}

fn parse_open<'a>(
    mut iter: impl Iterator<Item = &'a String>,
    file: bool,
) -> Result<CommandKind, String> {
    let usage = if file {
        "usage: hide file open <path> [--beside] [--reveal] [--request-id <id>]"
    } else {
        "usage: hide diff open <path> [--beside] [--reveal] [--request-id <id>]"
    };
    if iter.next().map(String::as_str) != Some("open") {
        return Err(usage.to_owned());
    }
    let path = iter
        .next()
        .filter(|path| !path.is_empty() && !path.starts_with('-'))
        .ok_or(usage)?;
    let (mut beside, mut reveal, mut request_id) = (false, false, None);
    while let Some(option) = iter.next() {
        match option.as_str() {
            "--beside" if !beside => beside = true,
            "--reveal" if !reveal => reveal = true,
            "--request-id" if request_id.is_none() => {
                request_id = Some(
                    iter.next()
                        .filter(|id| !id.is_empty() && !id.starts_with('-'))
                        .ok_or(usage)?
                        .clone(),
                );
            }
            _ => return Err(usage.to_owned()),
        }
    }
    let action = if file {
        Action::OpenFile {
            path: path.clone(),
            beside,
            reveal,
        }
    } else {
        Action::OpenDiff {
            path: path.clone(),
            beside,
            reveal,
        }
    };
    Ok(CommandKind::WorkspaceAction { action, request_id })
}

fn parse_view<'a>(mut iter: impl Iterator<Item = &'a String>) -> Result<CommandKind, String> {
    let usage = "usage: hide view list | status <view-id> | select <view-id> [--reveal] [--request-id <id>] | close <view-id> [--request-id <id>] | split <view-id> --area <area-id> --edge left|right|up|down [--request-id <id>] | move <view-id> --area <area-id> --index <n> [--request-id <id>]";
    let verb = iter.next().map(String::as_str).ok_or(usage)?;
    if verb == "list" {
        return if iter.next().is_none() {
            Ok(CommandKind::ViewList)
        } else {
            Err(usage.to_owned())
        };
    }
    if verb == "status" {
        return match (iter.next(), iter.next()) {
            (Some(view_id), None) if !view_id.is_empty() && !view_id.starts_with('-') => {
                Ok(CommandKind::ViewStatus {
                    view_id: view_id.clone(),
                })
            }
            _ => Err(usage.to_owned()),
        };
    }
    let view_id = iter
        .next()
        .filter(|id| !id.starts_with('-'))
        .ok_or(usage)?
        .clone();
    let mut area_id = None;
    let mut edge = None;
    let mut index = None;
    let mut request_id = None;
    let mut reveal = false;
    while let Some(option) = iter.next() {
        if option == "--reveal" && !reveal {
            reveal = true;
            continue;
        }
        let value = iter.next().ok_or(usage)?;
        if value.is_empty() || value.starts_with('-') {
            return Err(usage.to_owned());
        }
        match option.as_str() {
            "--area" if area_id.is_none() => area_id = Some(value.clone()),
            "--edge" if edge.is_none() => {
                edge = Some(match value.as_str() {
                    "left" => Edge::Left,
                    "right" => Edge::Right,
                    "up" => Edge::Up,
                    "down" => Edge::Down,
                    _ => return Err(usage.to_owned()),
                })
            }
            "--index" if index.is_none() => {
                index = Some(value.parse::<usize>().map_err(|_| usage)?)
            }
            "--request-id" if request_id.is_none() => request_id = Some(value.clone()),
            _ => return Err(usage.to_owned()),
        }
    }
    let action = match verb {
        "select" if area_id.is_none() && edge.is_none() && index.is_none() => {
            Action::Select { view_id, reveal }
        }
        "close" if area_id.is_none() && edge.is_none() && index.is_none() && !reveal => {
            Action::Close { view_id }
        }
        "split" if index.is_none() && !reveal => Action::Split {
            view_id,
            area_id: area_id.ok_or(usage)?,
            edge: edge.ok_or(usage)?,
        },
        "move" if edge.is_none() && !reveal => Action::Move {
            view_id,
            area_id: area_id.ok_or(usage)?,
            index: index.ok_or(usage)?,
        },
        _ => return Err(usage.to_owned()),
    };
    Ok(CommandKind::WorkspaceAction { action, request_id })
}

fn parse_browser<'a>(mut iter: impl Iterator<Item = &'a String>) -> Result<CommandKind, String> {
    if iter.next().map(String::as_str) != Some("open") {
        return Err(BROWSER_USAGE.to_owned());
    }
    let (mut target, mut reveal, mut wait, mut request_id) = (None, false, false, None);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--reveal" if !reveal => reveal = true,
            "--wait" if !wait => wait = true,
            "--request-id" if request_id.is_none() => {
                request_id = Some(
                    iter.next()
                        .filter(|id| !id.is_empty() && !id.starts_with('-'))
                        .ok_or(BROWSER_USAGE)?
                        .clone(),
                )
            }
            _ if target.is_none() && !arg.starts_with('-') => target = Some(arg.clone()),
            _ => return Err(BROWSER_USAGE.to_owned()),
        }
    }
    let target = target.ok_or_else(|| BROWSER_USAGE.to_owned())?;
    Ok(CommandKind::BrowserOpen {
        target,
        reveal,
        wait,
        request_id,
    })
}

pub fn run(kind: CommandKind) -> Result<(), String> {
    if kind == CommandKind::Help {
        println!(
            "hide workspace info\nhide file open <path> [--beside] [--reveal] [--request-id <id>]\nhide diff open <path> [--beside] [--reveal] [--request-id <id>]\nhide browser open <url-or-path> [--reveal] [--wait] [--request-id <id>]\nhide view list\nhide view status <view-id>\nhide view select <view-id> [--reveal] [--request-id <id>]\nhide view close <view-id> [--request-id <id>]\nhide view split <view-id> --area <area-id> --edge left|right|up|down [--request-id <id>]\nhide view move <view-id> --area <area-id> --index <n> [--request-id <id>]\nEach Workspace command requires a live Hide renderer and a caller Hide can bind to a checkout, an attested Herdr pane or a shell inside a registered checkout; it never starts Hide."
        );
        return Ok(());
    }
    let env = match env::load() {
        Ok(env) => env,
        Err(_errors)
            if matches!(
                &kind,
                CommandKind::WorkspaceBootstrap
                    | CommandKind::WorkspaceInfo
                    | CommandKind::ViewList
                    | CommandKind::ViewStatus { .. }
                    | CommandKind::BrowserOpen { .. }
                    | CommandKind::WorkspaceAction { .. }
            ) =>
        {
            return workspace_refusal(
                "environment_invalid",
                "Check Hide environment settings and retry",
            );
        }
        Err(errors) => {
            return Err(errors
                .iter()
                .map(|error| format!("{}: {}", error.key, error.kind))
                .collect::<Vec<_>>()
                .join("\n"));
        }
    };
    match kind {
        CommandKind::Help => unreachable!("handled above"),
        CommandKind::Open => open(&env),
        CommandKind::Connect => connect_json(&env),
        CommandKind::Status { json: true } => status_json(&env),
        CommandKind::Status { json: false } => status(&env),
        CommandKind::Stop => stop(&env),
        CommandKind::Serve { keep_alive } => serve(env, keep_alive),
        CommandKind::Dev => dev(env),
        CommandKind::BrowserOpen {
            target,
            reveal,
            wait,
            request_id,
        } => browser_open(&env, &target, reveal, wait, request_id.as_deref()),
        CommandKind::WorkspaceBootstrap => {
            let reference = crate::workspace_cli::bootstrap(&env, false)?;
            println!("{}", serde_json::json!({"ok":true,"reference":reference}));
            Ok(())
        }
        CommandKind::WorkspaceInfo => workspace_query(&env, "info"),
        CommandKind::ViewList => workspace_query(&env, "view_list"),
        CommandKind::ViewStatus { view_id } => view_status(&env, &view_id),
        CommandKind::WorkspaceAction { action, request_id } => {
            workspace_action(&env, action, request_id.as_deref())
        }
    }
}

fn workspace_action(env: &Env, action: Action, request_id: Option<&str>) -> Result<(), String> {
    let answer = workspace_action_value(env, action, request_id)?;
    println!("{answer}");
    Ok(())
}

fn workspace_action_value(
    env: &Env,
    action: Action,
    request_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    let action = match action {
        Action::OpenFile {
            path,
            beside,
            reveal,
        } => Action::OpenFile {
            path: match absolute_caller_path(&path) {
                Ok(path) => path,
                Err(reason) => {
                    return workspace_refusal(&reason, "Check the current directory and retry");
                }
            },
            beside,
            reveal,
        },
        Action::OpenDiff {
            path,
            beside,
            reveal,
        } => Action::OpenDiff {
            path: match absolute_caller_path(&path) {
                Ok(path) => path,
                Err(reason) => {
                    return workspace_refusal(&reason, "Check the current directory and retry");
                }
            },
            beside,
            reveal,
        },
        other => other,
    };
    let request_id = match request_id {
        Some(id) if crate::workspace_cli::valid_request_id(id) => id.to_owned(),
        Some(_) => {
            return workspace_refusal(
                "invalid_request_id",
                "Use the request ID printed by an earlier hide command",
            );
        }
        None => match crate::workspace_cli::fresh_request_id() {
            Ok(id) => id,
            Err(reason) => return workspace_refusal(&reason, "Check the local runtime and retry"),
        },
    };
    let (reference, ephemeral) = match workspace_reference(env) {
        Ok(reference) => reference,
        Err(reason) => return workspace_action_before_send_refusal(&request_id, &reason),
    };
    let _reference_owner =
        ephemeral.then(|| crate::workspace_cli::OneShotReference(reference.clone()));
    let answer = match crate::workspace_cli::request_action(&reference, action, &request_id) {
        Ok(answer) => answer,
        Err(reason) => return workspace_action_refusal(&request_id, &reason),
    };
    if answer["ok"] == true {
        Ok(answer)
    } else {
        println!("{answer}");
        Err(answer["reason"]
            .as_str()
            .unwrap_or("workspace_action_failed")
            .to_owned())
    }
}

fn workspace_action_before_send_refusal<T>(request_id: &str, reason: &str) -> Result<T, String> {
    println!(
        "{}",
        serde_json::json!({
            "ok": false,
            "request_id": request_id,
            "reason": reason,
            "applied": "not_applied",
            "next_action": "Open Hide, reconnect this pane, and retry the same command",
        })
    );
    Err(reason.to_owned())
}

fn absolute_caller_path(path: &str) -> Result<String, String> {
    use std::path::{Component, PathBuf};
    let cwd = std::env::current_dir().map_err(|_| "cwd_unavailable".to_owned())?;
    let input = PathBuf::from(path);
    let mut normalized = PathBuf::new();
    for component in cwd.join(input).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    // A pane's cwd may be reached through a platform alias such as macOS
    // /var -> /private/var, while Herdr reports the checkout's real path.
    // Resolve the nearest existing ancestor so deleted diff paths still
    // retain their final components and the host can judge them in its root.
    let mut ancestor = normalized.as_path();
    let mut missing = Vec::new();
    while !ancestor.exists() {
        let name = ancestor
            .file_name()
            .ok_or_else(|| "path_unavailable".to_owned())?;
        missing.push(name.to_os_string());
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "path_unavailable".to_owned())?;
    }
    let mut resolved = ancestor
        .canonicalize()
        .map_err(|_| "path_unavailable".to_owned())?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved.to_string_lossy().into_owned())
}

fn workspace_action_refusal<T>(request_id: &str, reason: &str) -> Result<T, String> {
    let applied = if matches!(
        reason,
        "request_timeout" | "hide_unavailable" | "invalid_response"
    ) {
        "unknown"
    } else {
        "not_applied"
    };
    println!(
        "{}",
        serde_json::json!({
            "ok": false,
            "request_id": request_id,
            "reason": reason,
            "applied": applied,
            "next_action": format!("Reconnect Hide, inspect hide view list, or retry the same command with --request-id {request_id}"),
        })
    );
    Err(reason.to_owned())
}

fn workspace_reference(env: &Env) -> Result<(std::path::PathBuf, bool), String> {
    match std::env::var(env::HIDE_CAP_REF) {
        Ok(value) if !value.is_empty() => Ok((std::path::PathBuf::from(value), false)),
        Ok(_) => Err("invalid_reference".to_owned()),
        Err(_) => crate::workspace_cli::bootstrap(env, true).map(|path| (path, true)),
    }
}

fn workspace_query(env: &Env, query: &str) -> Result<(), String> {
    let answer = workspace_query_value(env, query, true)?;
    println!("{answer}");
    Ok(())
}

fn workspace_query_value(
    env: &Env,
    query: &str,
    report: bool,
) -> Result<serde_json::Value, String> {
    let (reference, ephemeral) = match workspace_reference(env) {
        Ok(reference) => reference,
        Err(reason) => {
            return query_refusal(&reason, "Run Hide, reconnect this pane, and retry", report);
        }
    };
    let _reference_owner =
        ephemeral.then(|| crate::workspace_cli::OneShotReference(reference.clone()));
    let answer = match crate::workspace_cli::request(&reference, query) {
        Ok(answer) => answer,
        Err(reason) => {
            return query_refusal(
                &reason,
                "Check Hide status, reconnect the pane, and retry",
                report,
            );
        }
    };
    if answer["ok"] == true {
        Ok(answer)
    } else {
        if report {
            println!("{answer}");
        }
        Err(answer["reason"]
            .as_str()
            .unwrap_or("workspace_request_failed")
            .to_owned())
    }
}

fn query_refusal<T>(reason: &str, next_action: &str, report: bool) -> Result<T, String> {
    if report {
        workspace_refusal(reason, next_action)
    } else {
        Err(reason.to_owned())
    }
}

fn workspace_refusal<T>(reason: &str, next_action: &str) -> Result<T, String> {
    println!(
        "{}",
        serde_json::json!({"ok":false,"reason":reason,"next_action":next_action})
    );
    Err(reason.to_owned())
}

fn view_status_value(env: &Env, view_id: &str, report: bool) -> Result<serde_json::Value, String> {
    let answer = workspace_query_value(env, "view_list", report)?;
    let view = answer["result"]["views"]
        .as_array()
        .and_then(|views| views.iter().find(|view| view["view_id"] == view_id));
    match view {
        Some(view) => {
            Ok(serde_json::json!({"ok":true,"context":answer["result"]["context"],"view":view}))
        }
        None => {
            query_refusal(
                "view_missing",
                "Run hide view list and choose a current View",
                report,
            )?;
            unreachable!()
        }
    }
}

fn view_status(env: &Env, view_id: &str) -> Result<(), String> {
    let answer = view_status_value(env, view_id, true)?;
    println!("{answer}");
    Ok(())
}

fn browser_open(
    env: &Env,
    target: &str,
    reveal: bool,
    wait: bool,
    request_id: Option<&str>,
) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|_| "cwd_unavailable".to_owned())?;
    let url = match crate::browser_cli::address(target, &cwd) {
        Ok(url) => url,
        Err(_) => {
            return workspace_refusal(
                "invalid_address",
                "Use an http, https, or readable checkout HTML address",
            );
        }
    };
    let answer = workspace_action_value(env, Action::OpenBrowser { url, reveal }, request_id)?;
    if !wait {
        println!("{answer}");
        return Ok(());
    }
    let view_id = answer["result"]["view_id"]
        .as_str()
        .ok_or("invalid_response")?;
    let requested_load = answer["result"]["load"]
        .as_u64()
        .ok_or("invalid_response")?;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = match view_status_value(env, view_id, false) {
            Ok(status) => status,
            Err(reason) => {
                println!(
                    "{}",
                    serde_json::json!({"ok":false,"request_id":answer["request_id"],"applied":"applied","reason":"page_status_unavailable","detail":reason,"view_id":view_id,"result":answer["result"],"next_action":format!("Run hide view status {view_id} or retry the same request ID")})
                );
                return Err("page_status_unavailable".to_owned());
            }
        };
        let page = &status["view"]["page"];
        if page["load"].as_u64() != Some(requested_load) {
            println!(
                "{}",
                serde_json::json!({"ok":false,"request_id":answer["request_id"],"applied":"applied","reason":"page_superseded","view_id":view_id,"page":page,"next_action":"Run hide view status or open the page again with a new request ID"})
            );
            return Err("page_superseded".to_owned());
        }
        match page["state"].as_str() {
            Some("loaded") => {
                println!(
                    "{}",
                    serde_json::json!({"ok":true,"request_id":answer["request_id"],"result":answer["result"],"page":page})
                );
                return Ok(());
            }
            Some("failed") => {
                println!(
                    "{}",
                    serde_json::json!({"ok":false,"request_id":answer["request_id"],"applied":"applied","reason":"page_failed","view_id":view_id,"page":page,"next_action":"Inspect the page failure and retry with a new request ID"})
                );
                return Err("page_failed".to_owned());
            }
            Some("disconnected" | "unsupported") => {
                println!(
                    "{}",
                    serde_json::json!({"ok":false,"request_id":answer["request_id"],"applied":"applied","reason":"page_unavailable","view_id":view_id,"page":page,"next_action":"Reconnect the Hide desktop window and run hide view status again"})
                );
                return Err("page_unavailable".to_owned());
            }
            Some("pending" | "loading") => {}
            _ => {
                println!(
                    "{}",
                    serde_json::json!({"ok":false,"request_id":answer["request_id"],"applied":"applied","reason":"page_status_unavailable","view_id":view_id,"result":answer["result"],"page":page,"next_action":format!("Run hide view status {view_id} or retry the same request ID")})
                );
                return Err("page_status_unavailable".to_owned());
            }
        }
        if std::time::Instant::now() >= deadline {
            println!(
                "{}",
                serde_json::json!({"ok":false,"request_id":answer["request_id"],"applied":"applied","reason":"page_wait_timeout","view_id":view_id,"page":page,"next_action":format!("Run hide view status {view_id} or retry the same request ID")})
            );
            return Err("page_wait_timeout".to_owned());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Why no daemon could be reached or started, in the categories a host shows.
#[derive(Debug, Eq, PartialEq)]
pub enum ConnectError {
    /// The daemon process could not start.
    StartFailed(String),
    /// A daemon was started but never answered its health check.
    NoResponse(String),
}

impl ConnectError {
    fn reason(&self) -> &'static str {
        match self {
            ConnectError::StartFailed(_) => "start_failed",
            ConnectError::NoResponse(_) => "no_response",
        }
    }

    fn detail(&self) -> &str {
        match self {
            ConnectError::StartFailed(detail) | ConnectError::NoResponse(detail) => detail,
        }
    }
}

/// The one discovery path: the live daemon the state file names, or a new
/// one started and waited for. `open` and `connect` both run it, so a host
/// that loads the shell itself can never start a second daemon.
fn connect(env: &Env) -> Result<DaemonState, ConnectError> {
    if let Some(state) = healthy_state(env) {
        return Ok(state);
    }
    spawn_daemon(env, false).map_err(ConnectError::StartFailed)?;
    wait_healthy(env).map_err(ConnectError::NoResponse)
}

fn open(env: &Env) -> Result<(), String> {
    let state = connect(env).map_err(|error| error.detail().to_owned())?;
    open_browser(&state)
}

fn connect_json(env: &Env) -> Result<(), String> {
    let (line, result) = match connect(env) {
        Ok(state) => (attached_json(&state, "ok"), Ok(())),
        Err(error) => (
            serde_json::json!({
                "ok": false,
                "reason": error.reason(),
                "detail": error.detail(),
            }),
            Err(format!("{}: {}", error.reason(), error.detail())),
        ),
    };
    println!("{line}");
    result
}

/// The attach-only probe: never starts a daemon.
fn status_json(env: &Env) -> Result<(), String> {
    let line = match healthy_state(env) {
        Some(state) => attached_json(&state, "running"),
        None => serde_json::json!({ "running": false }),
    };
    println!("{line}");
    Ok(())
}

/// A live daemon as a host loads it. The URL carries the token in its hash,
/// exactly as `open` hands it to a browser.
fn attached_json(state: &DaemonState, flag: &str) -> serde_json::Value {
    serde_json::json!({
        flag: true,
        "url": daemon_url(state),
        "port": state.port,
        "pid": state.pid,
    })
}

fn daemon_url(state: &DaemonState) -> String {
    format!("http://127.0.0.1:{}/#token={}", state.port, state.token)
}

fn status(env: &Env) -> Result<(), String> {
    match healthy_state(env) {
        Some(state) => {
            let health = health_json(state.port)?;
            let clients = health
                .get("clients")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let idle = health
                .get("idle_remaining_secs")
                .and_then(|value| value.as_u64())
                .unwrap_or(env.idle_secs);
            println!(
                "pid {}  port {}  clients {}  idle {}s remaining",
                state.pid, state.port, clients, idle
            );
            Ok(())
        }
        None => {
            println!("hided is not running");
            Ok(())
        }
    }
}

fn stop(env: &Env) -> Result<(), String> {
    if let Some(state) = state_file::read_state(&env.state_dir).map_err(|e| e.to_string())? {
        let _ = send_signal(state.pid, libc::SIGTERM);
        for _ in 0..50 {
            if !pid_alive(state.pid) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if pid_alive(state.pid) {
            let _ = send_signal(state.pid, libc::SIGKILL);
        }
    }
    state_file::remove_state(&env.state_dir);
    Ok(())
}

fn serve(mut env: Env, keep_alive: bool) -> Result<(), String> {
    env.keep_alive = keep_alive || env.keep_alive;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("hide-serve")
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(crate::run_daemon(env))
}

fn dev(mut env: Env) -> Result<(), String> {
    env.keep_alive = true;
    if env.vite_origin.is_none() {
        env.vite_origin = Some("http://127.0.0.1:5173".into());
    }
    if env.bind.port() == 0 {
        env.bind = "127.0.0.1:9876".parse().expect("loopback");
    }
    println!(
        "HIDED_ORIGIN=http://127.0.0.1:{}  Vite origin {}",
        env.bind.port(),
        env.vite_origin.as_deref().unwrap_or("")
    );
    serve(env, true)
}

/// The `hided` beside this CLI's real file. A CLI reached through a symlink
/// (a desktop host's `HIDE_CLI_PATH`, a PATH entry) resolves to the build it
/// names; there is no other daemon to run, and running this CLI in its place
/// would start one more CLI per attempt, each starting the next.
fn daemon_binary() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|error| error.to_string())?;
    exe.parent()
        .map(|dir| dir.join("hided"))
        .filter(|path| path.is_file())
        .ok_or_else(|| format!("no hided beside {}", exe.display()))
}

fn spawn_daemon(env: &Env, keep_alive: bool) -> Result<(), String> {
    let mut command = Command::new(daemon_binary()?);
    command.env("HIDE_STATE_DIR", &env.state_dir);
    if keep_alive {
        command.env("HIDE_KEEP_ALIVE", "1");
    }
    if let Some(socket) = &env.herdr_socket_path {
        command.env("HERDR_SOCKET_PATH", socket);
    }
    if let Some(bin) = &env.herdr_bin_path {
        command.env("HERDR_BIN_PATH", bin);
    }
    if let Some(origin) = &env.vite_origin {
        command.env("HIDE_VITE_ORIGIN", origin);
    }
    let child = spawn_owned(&mut command).map_err(|error| error.to_string())?;
    std::mem::forget(child);
    Ok(())
}

fn wait_healthy(env: &Env) -> Result<DaemonState, String> {
    for _ in 0..100 {
        if let Some(state) = healthy_state(env) {
            return Ok(state);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("hided did not become healthy within 10s".into())
}

fn healthy_state(env: &Env) -> Option<DaemonState> {
    let state = state_file::read_state(&env.state_dir).ok().flatten()?;
    if !pid_alive(state.pid) {
        return None;
    }
    health_json(state.port).ok()?;
    Some(state)
}

fn health_json(port: u16) -> Result<serde_json::Value, String> {
    let url = format!("http://127.0.0.1:{port}/health");
    let output = Command::new("/usr/bin/curl")
        .args(["-fsS", "--max-time", "1", &url])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("health check failed".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

fn open_browser(state: &DaemonState) -> Result<(), String> {
    let url = daemon_url(state);
    Command::new("/usr/bin/open")
        .arg(&url)
        .status()
        .map_err(|error| error.to_string())?;
    println!("{url}");
    Ok(())
}

fn pid_alive(pid: u32) -> bool {
    send_signal(pid, 0).is_ok()
}

fn send_signal(pid: u32, signal: i32) -> io::Result<()> {
    let result = unsafe { libc::kill(pid as i32, signal) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_serve_keep_alive() {
        let args = ["hide", "serve", "--keep-alive"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        assert_eq!(
            parse_args(&args).unwrap(),
            CommandKind::Serve { keep_alive: true }
        );
    }

    #[test]
    fn parse_connect_and_status_json() {
        let parse = |line: &[&str]| {
            let args = line.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
            parse_args(&args).unwrap()
        };
        assert_eq!(parse(&["hide", "connect"]), CommandKind::Connect);
        assert_eq!(
            parse(&["hide", "status", "--json"]),
            CommandKind::Status { json: true }
        );
        assert_eq!(
            parse(&["hide", "status"]),
            CommandKind::Status { json: false }
        );
    }

    #[test]
    fn attached_json_carries_the_token_url() {
        let state = DaemonState {
            pid: 42,
            port: 7001,
            token: "abc".into(),
            socket: None,
            started_at: "now".into(),
        };
        assert_eq!(
            attached_json(&state, "ok"),
            serde_json::json!({
                "ok": true,
                "url": "http://127.0.0.1:7001/#token=abc",
                "port": 7001,
                "pid": 42,
            })
        );
    }

    #[test]
    fn parse_browser_open() {
        let parse = |line: &[&str]| {
            let args = line.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
            parse_args(&args)
        };
        assert_eq!(
            parse(&["hide", "browser", "open", "index.html"]),
            Ok(CommandKind::BrowserOpen {
                target: "index.html".into(),
                reveal: false,
                wait: false,
                request_id: None,
            })
        );
        assert_eq!(
            parse(&[
                "hide",
                "browser",
                "open",
                "localhost:3000",
                "--reveal",
                "--wait",
                "--request-id",
                "1234567890000-abc",
            ]),
            Ok(CommandKind::BrowserOpen {
                target: "localhost:3000".into(),
                reveal: true,
                wait: true,
                request_id: Some("1234567890000-abc".into()),
            })
        );
        for bad in [
            &["hide", "browser"][..],
            &["hide", "browser", "close", "a"],
            &["hide", "browser", "open"],
            &["hide", "browser", "open", "a", "b"],
            &["hide", "browser", "open", "a", "--pane"],
            &[
                "hide", "browser", "open", "a", "--pane", "p1", "--pane", "p2",
            ],
        ] {
            assert_eq!(parse(bad), Err(BROWSER_USAGE.to_owned()), "{bad:?}");
        }
    }

    #[test]
    fn view_commands_accept_one_view_and_no_workspace_override() {
        let parse = |line: &[&str]| {
            parse_args(&line.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
        };
        assert_eq!(
            parse(&[
                "hide",
                "view",
                "split",
                "d2",
                "--area",
                "a1",
                "--edge",
                "right",
                "--request-id",
                "1234567890000-abc"
            ]),
            Ok(CommandKind::WorkspaceAction {
                action: Action::Split {
                    view_id: "d2".into(),
                    area_id: "a1".into(),
                    edge: Edge::Right
                },
                request_id: Some("1234567890000-abc".into()),
            })
        );
        assert!(parse(&["hide", "view", "select", "d2", "--workspace", "other"]).is_err());
        assert!(parse(&["hide", "view", "move", "d2", "--area", "a1"]).is_err());
        assert!(
            parse(&[
                "hide", "view", "split", "d2", "--area", "a1", "--edge", "right", "--edge", "left"
            ])
            .is_err()
        );
    }

    #[test]
    fn file_and_diff_open_parse_scoped_paths_and_reject_target_overrides() {
        let parse = |line: &[&str]| {
            parse_args(&line.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
        };
        assert_eq!(
            parse(&["hide", "file", "open", "src/lib.rs", "--beside"]),
            Ok(CommandKind::WorkspaceAction {
                action: Action::OpenFile {
                    path: "src/lib.rs".into(),
                    beside: true,
                    reveal: false,
                },
                request_id: None,
            })
        );
        assert_eq!(
            parse(&["hide", "diff", "open", "src/lib.rs", "--reveal"]),
            Ok(CommandKind::WorkspaceAction {
                action: Action::OpenDiff {
                    path: "src/lib.rs".into(),
                    beside: false,
                    reveal: true,
                },
                request_id: None,
            })
        );
        assert!(parse(&["hide", "file", "open", "src/lib.rs", "--workspace", "other"]).is_err());
        assert!(parse(&["hide", "diff", "open", "src/lib.rs", "--beside", "--beside"]).is_err());
    }

    #[test]
    fn parse_open_default() {
        let args = ["hide"].into_iter().map(String::from).collect::<Vec<_>>();
        assert_eq!(parse_args(&args).unwrap(), CommandKind::Open);
    }
}
