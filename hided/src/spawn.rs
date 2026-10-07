//! Owned process entrypoint for the CLI daemon. A file opener is the node's
//! (`hide_node::opener`).

use std::io;
use std::process::{Child, Command, Stdio};

/// Cap on resident hided children this CLI owns. Crossing it is a failure.
pub const MAX_DAEMON_CHILDREN: usize = 1;

/// Starts a deliberately detached `hided` daemon from the short-lived CLI.
/// The file opener (`hide_node::opener`) has the opposite lifetime policy.
/// The daemon leads its own process group, so an interrupt sent to the
/// terminal job or host that ran the CLI (`pnpm dev`, a desktop app killing
/// a timed-out `hide connect`) never reaches it: its lifetime is its own.
pub fn spawn_owned(command: &mut Command) -> io::Result<Child> {
    hide_platform::process::detach(command)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}
