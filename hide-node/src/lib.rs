//! The machine side of Hide (PRD core-host-node D-21).
//!
//! A node does the work on the machine it runs on and answers the core
//! through [`NodeLink`]. [`Local`] is the core's own machine, answered in the
//! same process.

use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use cap_std::fs::Dir;
use hide_herdr_client::{ApiConnector, LocalSocketConnector};
use hide_host::serve::{Env, KitPlace};
use hide_node_link::RootIdentity;
use hide_node_link::protocol::Call;
use hide_node_link::{LinkAnswer, LinkError, NodeLink};

pub mod diagnostics;
// The test writer of a script run as a program, shared with the crate it
// was written for rather than copied.
#[cfg(all(test, unix))]
#[path = "../../hide-host/src/executable_fixture.rs"]
mod executable_fixture;
pub mod opener;
pub mod pane_proof;
pub mod ssh;

/// The machine this process runs on, answered in place, for the account
/// home it was given.
#[derive(Clone, Debug)]
pub struct Local {
    env: Env,
}

impl Local {
    /// The core's own node, answering for `home`: the home the core was
    /// configured with, which is not the process's for a daemon started with
    /// a private one. It installs no kit until [`Local::bundled`] says where
    /// the parts are.
    pub fn new(home: Option<PathBuf>) -> Self {
        Self {
            env: Env::standalone(home),
        }
    }

    /// The node runs from a desktop package whose resources folder,
    /// `kit_dir`, holds the kit's parts (`hide_kit::bundled_kit_dir`).
    pub fn bundled(mut self, kit_dir: PathBuf) -> Self {
        self.env.kit = KitPlace::Bundled(kit_dir);
        self
    }

    /// This process's own account, as a device's helper answers. Its stop
    /// is its own: closing it ends the work it started, never the work of
    /// another node value in the same process, which a helper's process-wide
    /// stop would.
    pub fn of_process() -> Self {
        let mut env = Env::of_process();
        env.stop = Arc::default();
        Self { env }
    }
}

/// This machine's connection to the Herdr server listening at `socket`.
/// A socket file that is not there answers `ApiError::NotRunning`, so the
/// core reads a stopped server from the answer, never from the disk.
pub fn herdr(socket: &std::path::Path) -> Arc<dyn ApiConnector> {
    Arc::new(LocalSocketConnector::new(socket))
}

/// Checkout roots the daemon opened under its pinned registrations, held open
/// while the core uses their identities, so a folder put at a root's path
/// later cannot take the identity of the one that was opened.
#[derive(Debug, Default)]
pub struct HeldRoots {
    _held: Vec<Dir>,
}

/// Holds `roots` and reads each one's identity from its handle; a root whose
/// identity cannot be read is named without one, and its first request pins
/// it.
pub fn hold_roots(
    roots: Vec<(PathBuf, File)>,
) -> (HeldRoots, Vec<(PathBuf, Option<RootIdentity>)>) {
    let mut held = Vec::with_capacity(roots.len());
    let mut identities = Vec::with_capacity(roots.len());
    for (path, file) in roots {
        let dir = Dir::from_std_file(file);
        identities.push((path, hide_host::root::identity_of(&dir).ok()));
        held.push(dir);
    }
    (HeldRoots { _held: held }, identities)
}

impl NodeLink for Local {
    fn call(&self, call: Call, _timeout: Duration) -> Result<LinkAnswer, LinkError> {
        hide_host::serve::handle_in(call, &self.env)
            .map(LinkAnswer::Parsed)
            .map_err(LinkError::Refused)
    }

    fn call_with_progress(
        &self,
        call: Call,
        _timeout: Duration,
        progress: &mut dyn FnMut(serde_json::Value) -> bool,
    ) -> Result<LinkAnswer, LinkError> {
        hide_host::serve::handle_with_progress(call, &self.env, progress)
            .map(LinkAnswer::Parsed)
            .map_err(LinkError::Refused)
    }

    fn in_process(&self) -> bool {
        true
    }

    /// The core is going away: a kit step still running ends the child it
    /// waits on rather than keep the process alive after the core.
    fn close(&self, _reason: &str) {
        self.env.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A daemon started with a private home must never answer from the
    /// operator's: the node reports and uses the home it was given.
    #[test]
    fn the_own_node_answers_for_the_home_it_was_given() {
        let given = PathBuf::from("/nonexistent/private-home");
        let answer = Local::new(Some(given.clone()))
            .call(Call::Hello, Duration::from_secs(5))
            .unwrap();
        let LinkAnswer::Parsed(hello) = answer else {
            panic!("an in-process answer is parsed");
        };
        assert_eq!(hello["home"], given.to_string_lossy().as_ref());
        assert_ne!(
            std::env::var_os("HOME").map(PathBuf::from),
            Some(given),
            "the test's home differs from the process's"
        );

        let refused = Local::new(None).call(
            Call::LinkFiles {
                since_unix_ms: 0,
                until_unix_ms: None,
            },
            Duration::from_secs(5),
        );
        assert!(
            matches!(&refused, Err(LinkError::Refused(error)) if error.message == "links_home_unavailable"),
            "{refused:?}"
        );
    }

    /// Closing one node value ends only its own work: a core dropped in the
    /// same process must not end the Git watch another node value runs.
    #[test]
    fn closing_one_node_leaves_another_nodes_watch_running() {
        use hide_node_link::worktrees::GitWatchReport;
        let repository = tempfile::tempdir().unwrap();
        let common = repository.path().canonicalize().unwrap();
        std::fs::create_dir_all(common.join("refs/heads")).unwrap();
        let closed = Local::of_process();
        let running = Local::of_process();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut wrote = false;
        let mut changed = false;
        running
            .call_with_progress(
                Call::GitWatch {
                    common_dirs: vec![common.to_string_lossy().into_owned()],
                },
                Duration::from_secs(30),
                &mut |report| {
                    if !wrote {
                        closed.close("the other core went away");
                        std::fs::write(common.join("refs/heads/main"), "0000\n").unwrap();
                        wrote = true;
                    }
                    changed |= matches!(
                        serde_json::from_value(report),
                        Ok(GitWatchReport::Changed { .. })
                    );
                    !changed && std::time::Instant::now() < deadline
                },
            )
            .unwrap();
        assert!(changed, "the running node's watch ended with the other");
    }
}
