//! The pane bootstrap's words for process identity (an `i32` pid, a start as
//! an optional number) over `hide-platform`, which answers each question for
//! the system it runs on. hided and a device's node (`panes`) ask the
//! same questions through here.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hide_herdr_client::{LocalSocketConnector, request_small_response_until};
use hide_platform::process;
use serde_json::{Value, json};

const HERDR_TIMEOUT: Duration = Duration::from_secs(2);

pub use hide_node_link::panes::PaneIdentity;

/// Whether `pid` is `shell_pid` or one of its descendants.
pub fn descends_from(pid: i32, shell_pid: i32) -> bool {
    match (u32::try_from(pid), u32::try_from(shell_pid)) {
        (Ok(pid), Ok(shell)) => process::descends_from(pid, shell),
        _ => false,
    }
}

/// When `pid` started, comparable only with another start of the same
/// system; `None` once the process is gone.
pub fn process_start(pid: i32) -> Option<u64> {
    process::start_time(u32::try_from(pid).ok()?).ok()
}

/// The peer's current directory as the system reports it, or `None` when the
/// process is gone or refuses inspection.
pub fn process_cwd(pid: i32) -> Option<PathBuf> {
    process::cwd_of(u32::try_from(pid).ok()?).ok()
}

/// The identity of the pane `pane_id` of the Herdr listening at `socket`.
pub fn inspect(socket: &Path, pane_id: &str) -> Result<PaneIdentity, &'static str> {
    inspect_until(socket, pane_id, Instant::now() + HERDR_TIMEOUT * 2)
}

/// The same pane proof under one caller-owned deadline, shared by both reads.
pub fn inspect_until(
    socket: &Path,
    pane_id: &str,
    deadline: Instant,
) -> Result<PaneIdentity, &'static str> {
    if !socket.is_absolute() || pane_id.is_empty() || pane_id.len() > 256 {
        return Err("invalid_request");
    }
    let connector = LocalSocketConnector::new(socket);
    let process = request_small_response_until(
        &connector,
        "pane.process_info",
        json!({"pane_id":pane_id}),
        stage_deadline(deadline)?,
    )
    .map_err(|_| "pane_unavailable")?;
    let shell = process
        .pointer("/process_info/shell_pid")
        .and_then(Value::as_u64)
        .filter(|pid| *pid > 0 && *pid <= i32::MAX as u64)
        .ok_or("pane_unavailable")? as i32;
    let started = process_start(shell).ok_or("pane_unavailable")?;
    let pane = request_small_response_until(
        &connector,
        "pane.get",
        json!({"pane_id":pane_id}),
        stage_deadline(deadline)?,
    )
    .map_err(|_| "pane_unavailable")?;
    let terminal_id = pane
        .pointer("/pane/terminal_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("pane_unavailable")?
        .to_owned();
    Ok(PaneIdentity {
        terminal_id,
        shell_pid: shell,
        shell_started: started,
    })
}

fn stage_deadline(deadline: Instant) -> Result<Instant, &'static str> {
    let now = Instant::now();
    if deadline <= now {
        return Err("pane_inspection_expired");
    }
    Ok(deadline.min(now + HERDR_TIMEOUT))
}
