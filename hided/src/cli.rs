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
    Status,
    Stop,
    Serve { keep_alive: bool },
    Dev,
}

pub fn parse_args(args: &[String]) -> Result<CommandKind, String> {
    let mut iter = args.iter().skip(1);
    match iter.next().map(String::as_str) {
        None | Some("open") => Ok(CommandKind::Open),
        Some("status") => Ok(CommandKind::Status),
        Some("stop") => Ok(CommandKind::Stop),
        Some("serve") => {
            let keep_alive = iter.any(|arg| arg == "--keep-alive");
            Ok(CommandKind::Serve { keep_alive })
        }
        Some("dev") => Ok(CommandKind::Dev),
        Some(other) => Err(format!("unknown command: {other}")),
    }
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
        CommandKind::Status => status(&env),
        CommandKind::Stop => stop(&env),
        CommandKind::Serve { keep_alive } => serve(env, keep_alive),
        CommandKind::Dev => dev(env),
    }
}

fn open(env: &Env) -> Result<(), String> {
    if let Some(state) = healthy_state(env) {
        return open_browser(&state);
    }
    confirm_coexist(env)?;
    spawn_daemon(env, false)?;
    let state = wait_healthy(env)?;
    open_browser(&state)
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

fn spawn_daemon(env: &Env, keep_alive: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let hided = exe
        .parent()
        .map(|dir| dir.join("hided"))
        .filter(|path| path.exists())
        .unwrap_or(exe);
    let mut command = Command::new(hided);
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
    let url = format!("http://127.0.0.1:{}/#token={}", state.port, state.token);
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
    fn parse_open_default() {
        let args = ["hide"].into_iter().map(String::from).collect::<Vec<_>>();
        assert_eq!(parse_args(&args).unwrap(), CommandKind::Open);
    }
}
