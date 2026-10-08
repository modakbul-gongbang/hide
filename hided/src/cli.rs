use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use herdr_core::workspace_control::{Action, Edge};
use hide_platform::process;

use crate::env::{self, Env};
use crate::spawn::spawn_owned;
use crate::state_file::{self, DaemonState};

#[derive(Debug, Eq, PartialEq)]
pub enum CommandKind {
    Delivery(herdr_core::delivery::Command),
    /// `hide factory ...`
    Factory(crate::factory_cli::FactoryRequest),
    Help,
    AgentSpawnHelp,
    /// `hide version [--json]`: this build's version, commit and contract.
    Version {
        json: bool,
    },
    /// `hide contract --json`: the contract of the commands tools call.
    Contract,
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
    BrowserConnect {
        display_id: Option<String>,
    },
    /// `hide browser <verb> <display> ...`: read or act on a page.
    BrowserPage(crate::browser_page::Command),
    /// `hide browser help`: the agent guide for the page commands.
    BrowserHelp,
    WorkspaceBootstrap,
    FactoryQuestionGuard {
        session: String,
        runtime: String,
    },
    WorkspaceInfo,
    ViewList,
    ViewStatus {
        view_id: String,
    },
    WorkspaceAction {
        action: Action,
        request_id: Option<String>,
    },
    /// `hide links`: a read of the link record (PRD link-graph D-19).
    Links(herdr_core::links::query::LinksQuery),
}

const LINKS_USAGE: &str = "usage: hide links pr <number> | issue <number> | branch <name> | session <id> [--all-projects]";

const BROWSER_USAGE: &str = "usage: hide browser open <url-or-path> [--reveal] [--wait] [--request-id <id>] | connect [--display <id>] | help | <command> <display> ... (see hide browser help)";

/// The version this build reports: `HIDE_VERSION` when it was built (a
/// package passes the version it ships), else the crate's own.
pub const VERSION: &str = env!("HIDE_BUILD_VERSION");

/// The commit this build was made from, when it was built from a checkout.
pub const COMMIT: Option<&str> = match env!("HIDE_BUILD_COMMIT").as_bytes() {
    [] => None,
    _ => Some(env!("HIDE_BUILD_COMMIT")),
};

pub fn parse_args(args: &[String]) -> Result<CommandKind, String> {
    let mut iter = args.iter().skip(1);
    match iter.next().map(String::as_str) {
        Some("help" | "--help" | "-h") if iter.next().is_none() => Ok(CommandKind::Help),
        Some("--version") if iter.next().is_none() => Ok(CommandKind::Version { json: false }),
        Some("version") => match (iter.next().map(String::as_str), iter.next()) {
            (None, _) => Ok(CommandKind::Version { json: false }),
            (Some("--json"), None) => Ok(CommandKind::Version { json: true }),
            _ => Err("usage: hide version [--json]".to_owned()),
        },
        Some("contract") => match (iter.next().map(String::as_str), iter.next()) {
            (Some("--json"), None) => Ok(CommandKind::Contract),
            _ => Err("usage: hide contract --json".to_owned()),
        },
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
        Some("agent") if args[2..] == ["spawn", "--help"] => Ok(CommandKind::AgentSpawnHelp),
        Some("agent") => {
            crate::cli_contract::admit(&args[1..])
                .map_err(|refusal| format!("{refusal}\n{}", crate::agent_cli::USAGE))?;
            crate::agent_cli::parse(iter).map(CommandKind::Delivery)
        }
        Some(topic @ ("request" | "inbox" | "watch")) => {
            crate::cli_contract::admit(&args[1..])
                .map_err(|refusal| format!("{refusal}\n{}", crate::delivery_cli::USAGE))?;
            crate::delivery_cli::parse(topic, iter).map(CommandKind::Delivery)
        }
        Some("factory") => crate::factory_cli::parse(iter).map(CommandKind::Factory),
        Some("browser") => parse_browser(iter),
        Some("workspace") => match iter.next().map(String::as_str) {
            Some("factory-question-guard") => {
                let words: Vec<&str> = iter.map(String::as_str).collect();
                match words.as_slice() {
                    ["--session", session, "--runtime", runtime]
                        if !session.is_empty() && session.len() <= 256
                            && !session.chars().any(char::is_control)
                            && matches!(*runtime, "claude-code" | "codex") =>
                        Ok(CommandKind::FactoryQuestionGuard { session: (*session).to_owned(), runtime: (*runtime).to_owned() }),
                    _ => Err("usage: hide workspace factory-question-guard --session <id> --runtime <claude-code|codex>".into()),
                }
            }
            Some("bootstrap") if iter.next().is_none() => Ok(CommandKind::WorkspaceBootstrap),
            Some("info") if iter.next().is_none() => Ok(CommandKind::WorkspaceInfo),
            _ => Err("usage: hide workspace info".to_owned()),
        },
        Some("file") => parse_open(iter, true),
        Some("diff") => parse_open(iter, false),
        Some("view") => parse_view(iter),
        Some("links") => parse_links(iter),
        Some(other) => Err(format!("unknown command: {other}")),
    }
}

fn parse_links<'a>(iter: impl Iterator<Item = &'a String>) -> Result<CommandKind, String> {
    use herdr_core::links::query::{LinksQuery, QueryTarget};
    let mut all_projects = false;
    let mut words = Vec::new();
    for word in iter {
        if word == "--all-projects" && !all_projects {
            all_projects = true;
        } else if word.starts_with('-') {
            return Err(LINKS_USAGE.to_owned());
        } else {
            words.push(word.as_str());
        }
    }
    let number = |value: &str| {
        value
            .strip_prefix('#')
            .unwrap_or(value)
            .parse::<u64>()
            .ok()
            .filter(|number| *number > 0)
    };
    let target = match words.as_slice() {
        ["pr", value] => number(value).map(|number| QueryTarget::Pr { number }),
        ["issue", value] => number(value).map(|number| QueryTarget::Issue { number }),
        ["branch", name] => Some(QueryTarget::Branch {
            name: (*name).to_owned(),
        }),
        ["session", id] => Some(QueryTarget::Session {
            id: (*id).to_owned(),
        }),
        _ => None,
    }
    .filter(QueryTarget::valid)
    .ok_or(LINKS_USAGE)?;
    Ok(CommandKind::Links(LinksQuery {
        target,
        all_projects,
    }))
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
        "select" if area_id.is_none() && edge.is_none() && index.is_none() => Action::Select {
            view_id,
            reveal,
            expected_browser_area: None,
        },
        "close" if area_id.is_none() && edge.is_none() && index.is_none() && !reveal => {
            Action::Close {
                view_id,
                expected_browser_area: None,
            }
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
    let verb = iter.next().map(String::as_str);
    if verb == Some("help") {
        return match iter.next() {
            None => Ok(CommandKind::BrowserHelp),
            Some(_) => Err(BROWSER_USAGE.to_owned()),
        };
    }
    if let Some(verb) = verb.filter(|verb| crate::browser_page::VERBS.contains(verb)) {
        return crate::browser_page::parse(verb, iter).map(CommandKind::BrowserPage);
    }
    if verb == Some("connect") {
        let display_id = match (iter.next().map(String::as_str), iter.next(), iter.next()) {
            (None, None, None) => None,
            (Some("--display"), Some(id), None) if !id.is_empty() && !id.starts_with('-') => {
                Some(id.clone())
            }
            _ => return Err(BROWSER_USAGE.to_owned()),
        };
        return Ok(CommandKind::BrowserConnect { display_id });
    }
    if verb != Some("open") {
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
    // Neither reads the environment nor needs a daemon.
    if let CommandKind::Version { json } = kind {
        let digest = crate::cli_contract::digest();
        if json {
            println!(
                "{}",
                serde_json::json!({"version": VERSION, "commit": COMMIT, "contract": digest})
            );
        } else {
            match COMMIT {
                Some(commit) => println!("hide {VERSION} {commit}"),
                None => println!("hide {VERSION}"),
            }
        }
        return Ok(());
    }
    if kind == CommandKind::Contract {
        println!("{}", crate::cli_contract::document());
        return Ok(());
    }
    if kind == CommandKind::AgentSpawnHelp {
        println!("{}", crate::agent_cli::SPAWN_HELP);
        return Ok(());
    }
    if kind == CommandKind::Help {
        println!("hide version [--json]\nhide contract --json");
        println!("{}", crate::delivery_cli::USAGE);
        println!("{}", crate::agent_cli::USAGE);
        println!("{}", hide_factory::command::USAGE);
        println!(
            "hide workspace info\nhide file open <path> [--beside] [--reveal] [--request-id <id>]\nhide diff open <path> [--beside] [--reveal] [--request-id <id>]\nhide browser open <url-or-path> [--reveal] [--wait] [--request-id <id>]\nhide browser connect [--display <id>]\nhide browser snapshot|click|fill|type|press|hover|drag|scroll|wait|screenshot|eval|console|network <display> ...\nhide browser help\nhide view list\nhide view status <view-id>\nhide view select <view-id> [--reveal] [--request-id <id>]\nhide view close <view-id> [--request-id <id>]\nhide view split <view-id> --area <area-id> --edge left|right|up|down [--request-id <id>]\nhide view move <view-id> --area <area-id> --index <n> [--request-id <id>]\nhide links pr <number> | issue <number> | branch <name> | session <id> [--all-projects]\nEach Workspace command requires a live Hide renderer and a caller Hide can bind to a checkout, an attested Herdr pane or a shell inside a registered checkout; it never starts Hide. hide links needs only the caller and a running Hide, and reads the caller's Project unless --all-projects is given."
        );
        return Ok(());
    }
    // The agent guide needs no daemon and no environment.
    if kind == CommandKind::BrowserHelp {
        print!("{}", crate::browser_page::HELP);
        return Ok(());
    }
    let env = match needed_keys(&kind).map_or_else(env::load, env::load_for) {
        Ok(env) => env,
        Err(_errors)
            if matches!(
                &kind,
                CommandKind::WorkspaceBootstrap
                    | CommandKind::FactoryQuestionGuard { .. }
                    | CommandKind::Delivery(_)
                    | CommandKind::Factory(_)
                    | CommandKind::WorkspaceInfo
                    | CommandKind::ViewList
                    | CommandKind::ViewStatus { .. }
                    | CommandKind::BrowserOpen { .. }
                    | CommandKind::BrowserConnect { .. }
                    | CommandKind::BrowserPage(_)
                    | CommandKind::WorkspaceAction { .. }
                    | CommandKind::Links(_)
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
        CommandKind::Delivery(command) => {
            if matches!(&command, herdr_core::delivery::Command::Agents { .. }) {
                crate::agent_cli::run(&env, command)
            } else {
                crate::delivery_cli::run(&env, command)
            }
        }
        CommandKind::Factory(request) => crate::factory_cli::run(&env, request),
        CommandKind::Help
        | CommandKind::AgentSpawnHelp
        | CommandKind::BrowserHelp
        | CommandKind::Version { .. }
        | CommandKind::Contract => unreachable!("handled above"),
        CommandKind::Open => open(&env),
        CommandKind::Connect => connect_json(&env),
        CommandKind::Status { json: true } => status_json(&env.state_dir),
        CommandKind::Status { json: false } => status(&env.state_dir, env.idle_secs),
        CommandKind::Stop => stop(&env.state_dir),
        CommandKind::Serve { keep_alive } => serve(env, keep_alive),
        CommandKind::Dev => dev(env),
        CommandKind::BrowserOpen {
            target,
            reveal,
            wait,
            request_id,
        } => browser_open(&env, &target, reveal, wait, request_id.as_deref()),
        CommandKind::BrowserConnect { display_id } => {
            let answer = browser_connect_value(&env, display_id.as_deref(), true)?;
            println!("{answer}");
            Ok(())
        }
        CommandKind::BrowserPage(command) => crate::browser_page::run(&env, command),
        CommandKind::WorkspaceBootstrap => {
            let reference = crate::workspace_cli::bootstrap(&env, false)?;
            println!("{}", serde_json::json!({"ok":true,"reference":reference}));
            Ok(())
        }
        CommandKind::WorkspaceInfo => workspace_query(&env, "info"),
        CommandKind::FactoryQuestionGuard { session, runtime } => {
            let credential = crate::workspace_cli::Credential::acquire(&env)?;
            let answer = crate::workspace_cli::request_factory_question_guard(
                &credential,
                &session,
                &runtime,
            )?;
            println!("{answer}");
            Ok(())
        }
        CommandKind::ViewList => workspace_query(&env, "view_list"),
        CommandKind::ViewStatus { view_id } => view_status(&env, &view_id),
        CommandKind::WorkspaceAction { action, request_id } => {
            workspace_action(&env, action, request_id.as_deref())
        }
        CommandKind::Links(query) => links(&env, &query),
    }
}

/// Prints the record's answer as one line; a refusal prints its reason and
/// next action and exits non-zero (B42, B43).
fn links(env: &Env, query: &herdr_core::links::query::LinksQuery) -> Result<(), String> {
    let mut credential = match crate::workspace_cli::Credential::acquire(env) {
        Ok(credential) => credential,
        Err(reason) => return query_refusal(&reason, bootstrap_next_action(&reason), true),
    };
    let answer = match crate::workspace_cli::request_links(&mut credential, query) {
        Ok(answer) => answer,
        Err(reason) => {
            return query_refusal(&reason, request_next_action(&reason), true);
        }
    };
    println!("{answer}");
    if answer["ok"] == true {
        Ok(())
    } else {
        Err(answer["reason"]
            .as_str()
            .unwrap_or("links_request_failed")
            .to_owned())
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
    let mut credential = match crate::workspace_cli::Credential::acquire(env) {
        Ok(credential) => credential,
        Err(reason) => return workspace_action_before_send_refusal(&request_id, &reason),
    };
    let answer = match crate::workspace_cli::request_action(&mut credential, action, &request_id) {
        Ok(answer) => answer,
        Err(reason) if bootstrap_refused(&reason) => {
            return workspace_action_before_send_refusal(&request_id, &reason);
        }
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

/// What the caller can do about a refused credential bootstrap. The daemon
/// answers the bootstrap with a reason only, so the CLI names the next step.
pub(crate) fn bootstrap_next_action(reason: &str) -> &'static str {
    match reason {
        "checkout_not_registered" => crate::pane_auth::CHECKOUT_NEXT_ACTION,
        "caller_unavailable" => "Retry from a live shell inside a registered project checkout",
        _ => "Open Hide, reconnect this pane, and retry the same command",
    }
}

/// A bootstrap refusal reaches a request when the reference `HIDE_CAP_REF`
/// named was gone and the bare bootstrap that replaces it was refused;
/// nothing was sent, and the bootstrap's next step still applies.
fn bootstrap_refused(reason: &str) -> bool {
    matches!(reason, "checkout_not_registered" | "caller_unavailable")
}

fn request_next_action(reason: &str) -> &'static str {
    if bootstrap_refused(reason) {
        bootstrap_next_action(reason)
    } else {
        "Check Hide status, reconnect the pane, and retry"
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
            "next_action": bootstrap_next_action(reason),
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
    let mut resolved = hide_platform::fs::identity::canonical(ancestor)
        .map_err(|_| "path_unavailable".to_owned())?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    hide_platform::path::to_wire(&resolved).map_err(|_| "path_unavailable".to_owned())
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
    workspace_query_options(env, query, None, report)
}

fn browser_connect_value(
    env: &Env,
    display_id: Option<&str>,
    report: bool,
) -> Result<serde_json::Value, String> {
    workspace_query_options(env, "browser_connect", display_id, report)
}

fn workspace_query_options(
    env: &Env,
    query: &str,
    display_id: Option<&str>,
    report: bool,
) -> Result<serde_json::Value, String> {
    let mut credential = match crate::workspace_cli::Credential::acquire(env) {
        Ok(credential) => credential,
        Err(reason) => {
            return query_refusal(&reason, bootstrap_next_action(&reason), report);
        }
    };
    let requested = if query == "browser_connect" {
        crate::workspace_cli::browser_connect(&mut credential, display_id)
    } else {
        crate::workspace_cli::request(&mut credential, query)
    };
    let answer = match requested {
        Ok(answer) => answer,
        Err(reason) => {
            return query_refusal(&reason, request_next_action(&reason), report);
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

#[allow(clippy::disallowed_methods)] // a production wait, not test code
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
    let mut answer = workspace_action_value(
        env,
        Action::OpenBrowser {
            url,
            reveal,
            area_id: None,
            new_target: false,
        },
        request_id,
    )?;
    let display_id = answer["result"]["view_id"]
        .as_str()
        .ok_or("invalid_response")?
        .to_owned();
    match browser_connect_value(env, Some(&display_id), false) {
        Ok(connection) => {
            for key in ["cdp_http_url", "browser_ws_url"] {
                answer["result"][key] = connection["result"][key].clone();
            }
        }
        Err(reason) => {
            // Opening already applied. Endpoint discovery cannot recast it as
            // a failed action and tempt a caller to create the page again.
            answer["result"]["browser_control"] = serde_json::json!({
                "state":"unavailable","reason":reason,
                "next_action":format!("Run hide browser connect --display {display_id} after the desktop window reconnects")
            });
        }
    }
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
    /// A daemon of another build runs here and this `hide` is not the app's,
    /// so it neither replaces nor attaches to it.
    OtherBuild(String),
    /// The daemon stopped because a stored file could not be made this
    /// machine's (PRD core-host-node B2); the host names the file.
    StateRefused { file: String, detail: String },
}

impl ConnectError {
    fn reason(&self) -> &'static str {
        match self {
            ConnectError::StartFailed(_) => "start_failed",
            ConnectError::NoResponse(_) => "no_response",
            ConnectError::OtherBuild(_) => "other_build",
            ConnectError::StateRefused { .. } => "state_refused",
        }
    }

    fn detail(&self) -> &str {
        match self {
            ConnectError::StartFailed(detail)
            | ConnectError::NoResponse(detail)
            | ConnectError::OtherBuild(detail)
            | ConnectError::StateRefused { detail, .. } => detail,
        }
    }
}

/// The one discovery path: the live daemon the state file names, or a new
/// one started and waited for. `open` and `connect` both run it, so a host
/// that loads the shell itself can never start a second daemon.
///
/// A live daemon of another build than the `hided` beside this CLI is
/// stopped and replaced (PRD labels-in-hided D-19) when this `hide` is the
/// app's own, the one in an app bundle's `Contents/Resources`: the app and
/// its daemon are always one build. Any other `hide` (a dev build, a copy)
/// refuses with the mismatch named and attaches to nothing, because the
/// installed app's daemon is the operator's. A daemon that will not stop
/// is a start failure. Only the daemon of this state folder is looked at,
/// so a dev or e2e daemon elsewhere is never replaced, and stopping it
/// leaves Herdr, its panes and their agents running.
///
/// The whole look-and-replace holds this state folder's connect lock, so
/// two connects never both stop a daemon and start their own.
fn connect(env: &Env) -> Result<DaemonState, ConnectError> {
    let build = crate::build_id::of_file(&daemon_binary().map_err(ConnectError::StartFailed)?)
        .map_err(ConnectError::StartFailed)?;
    #[cfg(unix)]
    move_legacy_state(env).map_err(ConnectError::StartFailed)?;
    let _serialized = state_file::lock_connect(&env.state_dir)
        .map_err(|error| ConnectError::StartFailed(format!("the connect lock: {error}")))?;
    if let Some((state, health)) = healthy_daemon(env) {
        let running = health.get("build").and_then(serde_json::Value::as_str);
        if running == Some(build.as_str()) {
            return Ok(state);
        }
        // Judged by the file this `hide` is, not the link it was invoked
        // through: the kit's command resolves into its package, and a
        // link that only looks like a package path has no authority.
        let from_package = std::env::current_exe()
            .and_then(|path| path.canonicalize())
            .ok()
            .as_deref()
            .and_then(hide_kit::bundled_kit_dir)
            .is_some();
        if !from_package {
            eprintln!(
                "{}",
                serde_json::json!({
                    "component": "hide", "kind": "daemon.other_build_refused",
                    "pid": state.pid, "running_build": running, "build": build,
                })
            );
            return Err(ConnectError::OtherBuild(format!(
                "a hided of another build is running (pid {}, build {}); this hide is not from a desktop package (build {build}) and neither replaces nor attaches to it",
                state.pid,
                running.unwrap_or("unknown"),
            )));
        }
        eprintln!(
            "{}",
            serde_json::json!({
                "component": "hide", "kind": "daemon.replacing",
                "pid": state.pid, "running_build": running, "build": build,
            })
        );
        stop_daemon(&env.state_dir, &state).map_err(|detail| {
            eprintln!(
                "{}",
                serde_json::json!({"component": "hide", "kind": "daemon.replace_failed", "pid": state.pid, "message": detail})
            );
            ConnectError::StartFailed(detail)
        })?;
    }
    let spawned_at = unix_ms();
    let mut daemon = spawn_daemon(env, false).map_err(ConnectError::StartFailed)?;
    let started = wait_healthy(env, &mut daemon, spawned_at);
    // The daemon outlives this command; nothing waits for it.
    std::mem::forget(daemon);
    started
}

/// Moves the legacy default state folder into place before anything looks
/// for a daemon (PRD hide-home-layout D-05). The daemon running from it is
/// the one that answers its own `/health` as the pid its state names, the
/// same rule that keeps a reused pid from ever being signalled.
#[cfg(unix)]
fn move_legacy_state(env: &Env) -> Result<(), String> {
    let Some(legacy) = env.legacy_state_dir.as_deref() else {
        return Ok(());
    };
    let moved = crate::state_move::move_legacy(legacy, &env.state_dir, |legacy| {
        let Some((state, _)) = healthy_daemon_in(legacy) else {
            return Ok(None);
        };
        stop_daemon(legacy, &state)?;
        Ok(Some(state.pid))
    })
    .map_err(|detail| format!("the state folder could not be moved: {detail}"))?;
    if let crate::state_move::Moved::Moved { stopped_pid } = moved {
        eprintln!(
            "{}",
            serde_json::json!({
                "component": "hide", "kind": "state.moved",
                "from": legacy.display().to_string(),
                "to": env.state_dir.display().to_string(),
                "stopped_pid": stopped_pid,
            })
        );
    }
    Ok(())
}

fn open(env: &Env) -> Result<(), String> {
    let state = connect(env).map_err(|error| error.detail().to_owned())?;
    open_browser(&state)
}

fn connect_json(env: &Env) -> Result<(), String> {
    let (line, result) = match connect(env) {
        Ok(state) => (attached_json(&state, "ok"), Ok(())),
        Err(error) => (
            match &error {
                ConnectError::StateRefused { file, .. } => serde_json::json!({
                    "ok": false,
                    "reason": error.reason(),
                    "detail": error.detail(),
                    "file": file,
                }),
                _ => serde_json::json!({
                    "ok": false,
                    "reason": error.reason(),
                    "detail": error.detail(),
                }),
            },
            Err(format!("{}: {}", error.reason(), error.detail())),
        ),
    };
    println!("{line}");
    result
}

/// The attach-only probe: never starts a daemon.
/// The environment keys a command reads. `None` is every key: the command acts
/// on the daemon's whole configuration, as the daemon does, and refuses on any
/// invalid one. `stop` and `status` only find a daemon, so an unrelated key
/// (an opener, a port) must not decide whether a running daemon can be asked
/// about or stopped.
fn needed_keys(kind: &CommandKind) -> Option<&'static [&'static str]> {
    match kind {
        CommandKind::Stop | CommandKind::Status { json: true } => Some(env::STATE_FOLDER_KEYS),
        CommandKind::Status { json: false } => Some(env::STATUS_KEYS),
        _ => None,
    }
}

fn status_json(state_dir: &Path) -> Result<(), String> {
    let line = match healthy_daemon_in(state_dir).map(|(state, _)| state) {
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

fn status(state_dir: &Path, idle_secs: u64) -> Result<(), String> {
    match healthy_daemon_in(state_dir).map(|(state, _)| state) {
        Some(state) => {
            let health = health_json(state.port, HEALTH_REQUEST)?;
            let clients = health
                .get("clients")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let idle = health
                .get("idle_remaining_secs")
                .and_then(|value| value.as_u64())
                .unwrap_or(idle_secs);
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

fn stop(state_dir: &Path) -> Result<(), String> {
    if let Some(state) = state_file::read_state(state_dir).map_err(|e| e.to_string())? {
        stop_daemon(state_dir, &state)?;
    }
    Ok(())
}

/// Whether `state`'s pid is still the daemon that recorded it. A pid another
/// process has reused (another account's, or a later daemon's) is not: it
/// reads as gone and is never signalled. A state with no recorded start comes
/// from an earlier build and is judged by liveness alone. A daemon that has
/// ended but that its starter has not reaped yet is gone too, although Linux
/// still lists its start time.
fn still_the_daemon(state: &DaemonState) -> bool {
    process::is_alive(state.pid)
        && state
            .pid_started
            .is_none_or(|recorded| process::start_time(state.pid).is_ok_and(|now| now == recorded))
}

/// Ends the daemon `state` names: SIGTERM and five seconds for its graceful
/// stop, which ends its AI requests and provider processes, then SIGKILL.
/// An error only when it is still alive after that.
#[allow(clippy::disallowed_methods)] // a production wait, not test code
fn stop_daemon(state_dir: &Path, state: &DaemonState) -> Result<(), String> {
    // A pid that is gone, or is another process now, has nothing to stop:
    // only the state is cleared.
    if !still_the_daemon(state) {
        state_file::forget_daemon(state_dir, state.pid);
        return Ok(());
    }
    // A damaged state's pid names no daemon (the platform layer refuses it),
    // so the state is only cleared.
    let asked = process::terminate(state.pid);
    if asked.is_err_and(|error| error.kind() == io::ErrorKind::InvalidInput) {
        state_file::forget_daemon(state_dir, state.pid);
        return Ok(());
    }
    for _ in 0..50 {
        if !still_the_daemon(state) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if still_the_daemon(state) {
        // The daemon leads its own group, so its provider processes go too.
        let _ = process::kill_tree(state.pid);
        for _ in 0..20 {
            if !still_the_daemon(state) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    if still_the_daemon(state) {
        return Err(format!(
            "the running hided (pid {}) did not stop",
            state.pid
        ));
    }
    // Only the state of the daemon stopped here: another connect may have
    // started its own since, and its state and lock stay.
    state_file::forget_daemon(state_dir, state.pid);
    Ok(())
}

fn serve(mut env: Env, keep_alive: bool) -> Result<(), String> {
    env.keep_alive = keep_alive || env.keep_alive;
    // The daemon runs inside this CLI here; its build is the `hided` this
    // CLI ships beside, the one `hide connect` compares against.
    env.build = Some(crate::build_id::of_file(&daemon_binary()?)?);
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

/// The `hided` beside this CLI's real file (`hided.exe` on Windows). A CLI
/// reached through a symlink (a desktop host's `HIDE_CLI_PATH`, a PATH entry)
/// resolves to the build it names; there is no other daemon to run, and
/// running this CLI in its place would start one more CLI per attempt, each
/// starting the next.
fn daemon_binary() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|error| error.to_string())?;
    exe.parent()
        .map(|dir| dir.join(format!("hided{}", std::env::consts::EXE_SUFFIX)))
        .filter(|path| path.is_file())
        .ok_or_else(|| format!("no hided beside {}", exe.display()))
}

fn spawn_daemon(env: &Env, keep_alive: bool) -> Result<std::process::Child, String> {
    // The daemon would refuse the same value after it is spawned; checking
    // here answers `start_failed` with the path instead of a ten-second
    // `no_response` that names nothing.
    if let Some(error) = env::herdr_bin_error(env) {
        return Err(error);
    }
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
    spawn_owned(&mut command).map_err(|error| error.to_string())
}

/// How long `hide connect` waits for the daemon it started to answer, on
/// the wall clock; the error names this same bound.
const HEALTHY_WITHIN: Duration = Duration::from_secs(10);

/// The longest one `/health` request may take.
const HEALTH_REQUEST: Duration = Duration::from_secs(1);

/// The pause between two looks at a daemon that has not answered yet.
const HEALTH_PAUSE: Duration = Duration::from_millis(100);

/// Waits for the daemon `hide connect` started. One that exits first ends
/// the wait at once, with the reason it logged when it refused its state
/// folder (PRD core-host-node B2), since its stderr goes nowhere.
#[allow(clippy::disallowed_methods)] // a production wait, not test code
fn wait_healthy(
    env: &Env,
    daemon: &mut std::process::Child,
    spawned_at: u64,
) -> Result<DaemonState, ConnectError> {
    let pid = daemon.id();
    let waited = wait_started(
        HEALTHY_WITHIN,
        std::time::Instant::now,
        std::thread::sleep,
        || daemon.try_wait().ok().flatten(),
        |timeout| probe_daemon(&env.state_dir, timeout).map(|(state, _)| state),
    );
    match waited {
        Ok(state) => Ok(state),
        Err(Waited::Timeout(detail)) => Err(ConnectError::NoResponse(detail)),
        Err(Waited::Exited(status)) => {
            let refusal = herdr_core::diagnostics::newest_record(
                &env.state_dir.join("core-state.json"),
                |record| refused_by(record, pid, spawned_at),
            );
            Err(match refusal {
                Some(record) => ConnectError::StateRefused {
                    file: record["file"].as_str().unwrap_or_default().to_owned(),
                    detail: record["reason"].as_str().unwrap_or_default().to_owned(),
                },
                None => ConnectError::StartFailed(format!(
                    "hided exited with {status} before it answered"
                )),
            })
        }
    }
}

/// The refusal this start's daemon logged: its pid, written no earlier
/// than it was spawned, so a record an earlier daemon with a reused pid
/// left in the same Logs file is not read as this one's.
fn refused_by(record: &serde_json::Value, pid: u32, spawned_at: u64) -> bool {
    record["kind"] == "node_migration.refused"
        && record["pid"].as_u64() == Some(u64::from(pid))
        && record["at_unix_ms"]
            .as_u64()
            .is_some_and(|at| at >= spawned_at)
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

enum Waited {
    Timeout(String),
    Exited(std::process::ExitStatus),
}

fn wait_started(
    within: Duration,
    now: impl Fn() -> std::time::Instant,
    mut pause: impl FnMut(Duration),
    mut exited: impl FnMut() -> Option<std::process::ExitStatus>,
    mut probe: impl FnMut(Duration) -> Result<DaemonState, String>,
) -> Result<DaemonState, Waited> {
    let deadline = now() + within;
    loop {
        let left = deadline.saturating_duration_since(now());
        let seen = match probe(left.min(HEALTH_REQUEST)) {
            Ok(state) => return Ok(state),
            Err(seen) => seen,
        };
        if let Some(status) = exited() {
            return Err(Waited::Exited(status));
        }
        let left = deadline.saturating_duration_since(now());
        if left.is_zero() {
            return Err(Waited::Timeout(format!(
                "hided did not become healthy within {within:?}; last waited on {seen}"
            )));
        }
        pause(left.min(HEALTH_PAUSE));
    }
}

/// Looks at the started daemon until it answers or `within` has passed on
/// `now`'s clock. No request may outlast the time left, so the wait ends at
/// the bound it names, and the error says what was last seen, so a slow
/// start names its stage.
#[cfg(test)]
fn wait_healthy_within(
    within: Duration,
    now: impl Fn() -> std::time::Instant,
    pause: impl FnMut(Duration),
    probe: impl FnMut(Duration) -> Result<DaemonState, String>,
) -> Result<DaemonState, String> {
    wait_started(within, now, pause, || None, probe).map_err(|waited| match waited {
        Waited::Timeout(detail) => detail,
        Waited::Exited(status) => format!("exited with {status}"),
    })
}

/// The live daemon of this state folder and what its `/health` answered.
/// The answer has to come from the process the state names: a pid reused
/// after a crash, with another daemon on the port, is a stale state that
/// `hide connect` must never signal.
fn healthy_daemon(env: &Env) -> Option<(DaemonState, serde_json::Value)> {
    healthy_daemon_in(&env.state_dir)
}

fn healthy_daemon_in(state_dir: &Path) -> Option<(DaemonState, serde_json::Value)> {
    probe_daemon(state_dir, HEALTH_REQUEST).ok()
}

/// The daemon `state_dir` names and its `/health`, asked within `timeout`,
/// or what stands in the way: no state yet, a pid that is not that daemon,
/// or a `/health` that failed or came from another process.
fn probe_daemon(
    state_dir: &Path,
    timeout: Duration,
) -> Result<(DaemonState, serde_json::Value), String> {
    let state = state_file::read_state(state_dir)
        .map_err(|error| format!("the daemon state, which could not be read: {error}"))?
        .ok_or_else(|| "the daemon to write its state".to_owned())?;
    if !still_the_daemon(&state) {
        return Err(format!(
            "pid {} the state names, which is not that daemon",
            state.pid
        ));
    }
    let health = health_json(state.port, timeout)
        .map_err(|error| format!("/health on port {}: {error}", state.port))?;
    let answered = health.get("pid").and_then(serde_json::Value::as_u64);
    if answered != Some(u64::from(state.pid)) {
        return Err(format!(
            "/health on port {}, which answered for pid {answered:?} rather than {}",
            state.port, state.pid
        ));
    }
    Ok((state, health))
}

/// The daemon's `/health`, asked in process: a forked `curl` exists only
/// where the system ships one at `/usr/bin/curl`, and Windows has none. The
/// request goes to this machine's loopback, so no proxy from the
/// environment may carry it, and a status other than 2xx is a failure.
fn health_json(port: u16, timeout: Duration) -> Result<serde_json::Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .proxy(None)
        .build()
        .into();
    let body = agent
        .get(&format!("http://127.0.0.1:{port}/health"))
        .call()
        .map_err(|error| error.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|error| error.to_string())?;
    serde_json::from_str(&body).map_err(|error| error.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_is_read_only_from_the_daemon_this_start_spawned() {
        let record = |pid: u32, at: u64| serde_json::json!({"kind": "node_migration.refused", "pid": pid, "at_unix_ms": at, "file": "/s/node.json"});
        assert!(refused_by(&record(42, 1_000), 42, 1_000));
        // Another daemon's, or one an earlier daemon with the same pid left.
        assert!(!refused_by(&record(43, 1_000), 42, 1_000));
        assert!(!refused_by(&record(42, 999), 42, 1_000));
        assert!(!refused_by(
            &serde_json::json!({"kind": "node_migration.refused", "pid": 42}),
            42,
            0
        ));
    }

    fn recorded(pid: u32, pid_started: Option<u64>) -> DaemonState {
        DaemonState {
            pid,
            port: 7001,
            token: "abc".into(),
            socket: None,
            started_at: "now".into(),
            pid_started,
        }
    }

    #[test]
    fn a_pid_reused_by_another_process_reads_as_gone_and_is_not_signalled() {
        // A live process stands in for the stranger that now holds the pid:
        // the state recorded a different start for it.
        let mut stranger = Command::new("sleep").arg("30").spawn().unwrap();
        let real_start = process::start_time(stranger.id()).unwrap();
        let replaced = recorded(stranger.id(), Some(real_start.wrapping_add(1)));
        assert!(!still_the_daemon(&replaced));

        let dir = tempfile::tempdir().unwrap();
        state_file::write_state(dir.path(), &replaced).unwrap();
        stop_daemon(dir.path(), &replaced).unwrap();
        assert!(
            stranger.try_wait().unwrap().is_none(),
            "the process that reused the pid was signalled"
        );
        assert!(
            state_file::read_state(dir.path()).unwrap().is_none(),
            "the stale state is cleared"
        );

        // The same process under the start the state recorded is the daemon.
        assert!(still_the_daemon(&recorded(stranger.id(), Some(real_start))));
        let _ = stranger.kill();
        let _ = stranger.wait();
    }

    #[test]
    fn a_daemon_that_never_answers_ends_the_wait_at_its_bound_and_names_what_it_waited_on() {
        let start = std::time::Instant::now();
        let clock = std::cell::Cell::new(start);
        let asked = std::cell::RefCell::new(Vec::new());
        let error = wait_healthy_within(
            HEALTHY_WITHIN,
            || clock.get(),
            |pause| clock.set(clock.get() + pause),
            // A `/health` that times out takes all the time it is given.
            |timeout| {
                let left = (start + HEALTHY_WITHIN).saturating_duration_since(clock.get());
                asked.borrow_mut().push((timeout, left));
                clock.set(clock.get() + timeout);
                Err("/health on port 7001: timed out".to_owned())
            },
        )
        .unwrap_err();
        assert_eq!(
            clock.get() - start,
            HEALTHY_WITHIN,
            "the wait ends at the bound it names"
        );
        assert!(
            asked
                .borrow()
                .iter()
                .all(|(timeout, left)| timeout <= left && *timeout <= HEALTH_REQUEST),
            "no request outlasts the time left: {:?}",
            asked.borrow()
        );
        assert_eq!(
            error,
            "hided did not become healthy within 10s; last waited on /health on port 7001: timed out"
        );
    }

    // The web e2e fixture runs `hide stop` on the daemon it started, and
    // cannot reap it while that runs.
    #[cfg(unix)]
    #[test]
    fn an_ended_daemon_its_starter_has_not_reaped_reads_as_stopped() {
        let mut ended = Command::new("sleep").arg("30").spawn().unwrap();
        let state = recorded(ended.id(), Some(process::start_time(ended.id()).unwrap()));
        let dir = tempfile::tempdir().unwrap();
        state_file::write_state(dir.path(), &state).unwrap();
        ended.kill().unwrap();
        stop_daemon(dir.path(), &state).unwrap();
        assert!(
            state_file::read_state(dir.path()).unwrap().is_none(),
            "the stopped daemon's state is cleared"
        );
        ended.wait().unwrap();
    }

    #[test]
    fn a_state_with_no_recorded_start_is_judged_by_liveness() {
        assert!(still_the_daemon(&recorded(std::process::id(), None)));
        assert!(!still_the_daemon(&recorded(0, None)));
    }

    #[test]
    fn spawn_alone_supports_subcommand_help_and_rejects_no_watch_before_delivery() {
        let parse = |args: &[&str]| {
            parse_args(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
        };
        assert_eq!(
            parse(&["hide", "agent", "spawn", "--help"]).unwrap(),
            CommandKind::AgentSpawnHelp
        );
        for verb in ["register", "list", "show", "end"] {
            assert!(
                parse(&["hide", "agent", verb, "--help"])
                    .unwrap_err()
                    .contains("Unknown flag")
            );
        }
        let error = parse(&["hide", "agent", "spawn", "--no-watch"]).unwrap_err();
        assert!(error.starts_with("Unknown flag for hide agent spawn: --no-watch\n"));
        assert!(error.contains(crate::agent_cli::USAGE));
    }

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
    fn factory_question_guard_requires_one_native_session_and_known_runtime() {
        let parse = |args: &[&str]| {
            parse_args(
                &args
                    .iter()
                    .map(|word| (*word).to_owned())
                    .collect::<Vec<_>>(),
            )
        };
        for runtime in ["claude-code", "codex"] {
            assert_eq!(
                parse(&[
                    "hide",
                    "workspace",
                    "factory-question-guard",
                    "--session",
                    "native-1",
                    "--runtime",
                    runtime
                ])
                .unwrap(),
                CommandKind::FactoryQuestionGuard {
                    session: "native-1".into(),
                    runtime: runtime.into()
                }
            );
        }
        for args in [
            vec!["hide", "workspace", "factory-question-guard"],
            vec![
                "hide",
                "workspace",
                "factory-question-guard",
                "--session",
                "",
                "--runtime",
                "codex",
            ],
            vec![
                "hide",
                "workspace",
                "factory-question-guard",
                "--session",
                "s1",
                "--runtime",
                "foreign",
            ],
            vec![
                "hide",
                "workspace",
                "factory-question-guard",
                "--session",
                "s1",
                "--runtime",
                "codex",
                "--pane",
                "w1:p1",
            ],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
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
    fn browser_connect_has_no_workspace_override() {
        let parse = |args: &[&str]| {
            parse_args(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
        };
        assert_eq!(
            parse(&["hide", "browser", "connect"]).unwrap(),
            CommandKind::BrowserConnect { display_id: None }
        );
        assert_eq!(
            parse(&["hide", "browser", "connect", "--display", "browser-a"]).unwrap(),
            CommandKind::BrowserConnect {
                display_id: Some("browser-a".into())
            }
        );
        for args in [
            vec!["hide", "browser", "connect", "--display"],
            vec!["hide", "browser", "connect", "--workspace", "other"],
            vec![
                "hide",
                "browser",
                "connect",
                "--display",
                "browser-a",
                "--reveal",
            ],
        ] {
            assert!(parse(&args).is_err());
        }
    }

    #[test]
    fn attached_json_carries_the_token_url() {
        let state = DaemonState {
            pid: 42,
            port: 7001,
            token: "abc".into(),
            socket: None,
            started_at: "now".into(),
            pid_started: None,
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
    fn links_reads_one_target_and_has_no_write_command() {
        use herdr_core::links::query::{LinksQuery, QueryTarget};
        let parse = |line: &[&str]| {
            parse_args(&line.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
        };
        assert_eq!(
            parse(&["hide", "links", "pr", "#317"]),
            Ok(CommandKind::Links(LinksQuery {
                target: QueryTarget::Pr { number: 317 },
                all_projects: false,
            }))
        );
        assert_eq!(
            parse(&["hide", "links", "--all-projects", "branch", "feat/x"]),
            Ok(CommandKind::Links(LinksQuery {
                target: QueryTarget::Branch {
                    name: "feat/x".into()
                },
                all_projects: true,
            }))
        );
        assert_eq!(
            parse(&["hide", "links", "session", "a1b2"]),
            Ok(CommandKind::Links(LinksQuery {
                target: QueryTarget::Session { id: "a1b2".into() },
                all_projects: false,
            }))
        );
        for bad in [
            &["hide", "links"][..],
            &["hide", "links", "pr", "0"],
            &["hide", "links", "pr", "abc"],
            &["hide", "links", "issue"],
            &["hide", "links", "add", "pr", "1"],
            &["hide", "links", "pr", "1", "--project", "other"],
            &[
                "hide",
                "links",
                "pr",
                "1",
                "--all-projects",
                "--all-projects",
            ],
        ] {
            assert_eq!(parse(bad), Err(LINKS_USAGE.to_owned()), "{bad:?}");
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
