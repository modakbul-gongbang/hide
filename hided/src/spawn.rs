//! The only function that starts a child process in this crate.

use std::io;
use std::process::{Child, Command, Stdio};

/// Cap on resident hided children this CLI owns. Crossing it is a failure.
pub const MAX_DAEMON_CHILDREN: usize = 1;

/// Starts `hided` so a closed pipe from this parent ends the child.
///
/// macOS has no parent-death signal. The child inherits an extra pipe; its
/// owner-thread loop (and axum) die when the parent is gone if the child
/// watches that pipe, and the parent still holds the `Child` to kill on
/// drop. Tests assert the count stays at one.
pub fn spawn_owned(command: &mut Command) -> io::Result<Child> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

#[cfg(test)]
mod tests {
    #[test]
    fn spawn_owned_is_the_only_command_spawn() {
        let source = include_str!("spawn.rs");
        let lib = include_str!("cli.rs");
        assert!(
            lib.contains("spawn_owned"),
            "CLI must start children through spawn_owned"
        );
        assert!(source.contains("pub fn spawn_owned"));
    }
}
