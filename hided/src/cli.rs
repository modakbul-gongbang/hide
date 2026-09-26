use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::coexist::{self, Decision};
use crate::env::{self, Env};
use crate::spawn::spawn_owned;
use crate::state_file::{self, DaemonState};

#[derive(Debug, Eq, PartialEq)]
pub enum CommandKind {
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
    /// `hide browser open <url-or-path> [--pane <id>]`: shows a page in a
    /// View area of the running hide. Attach-only; it never starts a daemon.
    BrowserOpen {
        target: String,
        pane: Option<String>,
    },
}

const BROWSER_USAGE: &str = "usage: hide browser open <url-or-path> [--pane <pane-id>]";

pub fn parse_args(args: &[String]) -> Result<CommandKind, String> {
    let mut iter = args.iter().skip(1);
    match iter.next().map(String::as_str) {
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
        Some(other) => Err(format!("unknown command: {other}")),
    }
}

fn parse_browser<'a>(mut iter: impl Iterator<Item = &'a String>) -> Result<CommandKind, String> {
    if iter.next().map(String::as_str) != Some("open") {
        return Err(BROWSER_USAGE.to_owned());
    }
    let (mut target, mut pane) = (None, None);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--pane" => match iter.next() {
                Some(id) if !id.is_empty() && pane.is_none() => pane = Some(id.clone()),
                _ => return Err(BROWSER_USAGE.to_owned()),
            },
            _ if target.is_none() => target = Some(arg.clone()),
            _ => return Err(BROWSER_USAGE.to_owned()),
        }
    }
    let target = target.ok_or_else(|| BROWSER_USAGE.to_owned())?;
    Ok(CommandKind::BrowserOpen { target, pane })
}

pub fn run(kind: CommandKind) -> Result<(), String> {
    let env = env::load().map_err(|errors| {
        errors
            .iter()
            .map(|error| format!("{}: {}", error.key, error.kind))
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    match kind {
        CommandKind::Open => open(&env),
        CommandKind::Connect => connect_json(&env),
        CommandKind::Status { json: true } => status_json(&env),
        CommandKind::Status { json: false } => status(&env),
        CommandKind::Stop => stop(&env),
        CommandKind::Serve { keep_alive } => serve(env, keep_alive),
        CommandKind::Dev => dev(env),
        CommandKind::BrowserOpen { target, pane } => browser_open(&env, &target, pane),
    }
}

/// Prints the core's receipt, or the refusal, as one JSON line, and fails on
/// anything but an opened page. A page has nowhere to show without a running
/// hide, so this attaches to one and never starts it.
fn browser_open(env: &Env, target: &str, pane: Option<String>) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let url = crate::browser_cli::address(target, &cwd)?;
    let state = healthy_state(env).ok_or_else(|| "hide is not running".to_owned())?;
    let pane = pane.or_else(|| env.pane_id.clone());
    let mut id = [0_u8; 16];
    getrandom::getrandom(&mut id).map_err(|error| error.to_string())?;
    let payload = crate::browser_cli::payload(&url, pane.as_deref(), &hex::encode(id));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let answer = runtime.block_on(crate::browser_cli::request(&state, payload))?;
    println!("{answer}");
    if answer["ok"] == true {
        Ok(())
    } else {
        Err("the page did not open".to_owned())
    }
}

/// Why no daemon could be reached or started, in the categories a host shows.
#[derive(Debug, Eq, PartialEq)]
pub enum ConnectError {
    /// The coexistence check refused, or the daemon process could not start.
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
    confirm_coexist(env).map_err(ConnectError::StartFailed)?;
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
    confirm_coexist(&env)?;
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
    confirm_coexist(&env)?;
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
    for _ in 0..50 {
        if let Some(state) = healthy_state(env) {
            return Ok(state);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("hided did not become healthy within 5s".into())
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

/// PRD B5: warn only when hided would attach to the socket the Swift shell
/// already holds; an isolated socket never prompts. Without a TTY the shared
/// case refuses instead of asking.
fn confirm_coexist(env: &Env) -> Result<(), String> {
    let target = env.herdr_socket_path.as_deref().map(Path::new);
    let outcome = coexist::classify(target, coexist::find_swift_shell());
    let prompt = match coexist::decide(outcome, io::stdin().is_terminal()) {
        Decision::Proceed => return Ok(()),
        Decision::Refuse(message) => return Err(message),
        Decision::Ask(prompt) => prompt,
    };
    eprint!("{prompt}");
    let _ = io::stderr().flush();
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    match line.trim() {
        "y" | "Y" | "yes" => Ok(()),
        _ => Err("aborted".into()),
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
                pane: None
            })
        );
        assert_eq!(
            parse(&[
                "hide",
                "browser",
                "open",
                "--pane",
                "w1:p2",
                "localhost:3000"
            ]),
            Ok(CommandKind::BrowserOpen {
                target: "localhost:3000".into(),
                pane: Some("w1:p2".into())
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
    fn parse_open_default() {
        let args = ["hide"].into_iter().map(String::from).collect::<Vec<_>>();
        assert_eq!(parse_args(&args).unwrap(), CommandKind::Open);
    }
}
