//! The pane bootstrap's words for process identity (an `i32` pid, a start as
//! an optional number) over `hide-platform`, which answers each question for
//! the system it runs on. hided and the workspace bridge on a device ask the
//! same questions through here.

use std::os::fd::AsFd;
use std::path::PathBuf;

use hide_platform::{ipc, process};

/// The pid of the process at the other end of `stream`, when the system
/// reports it.
pub fn peer_pid(stream: &impl AsFd) -> Option<i32> {
    ipc::peer_pid_of_fd(stream)
        .ok()
        .and_then(|pid| i32::try_from(pid).ok())
}

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
