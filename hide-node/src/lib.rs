//! The machine side of Hide (PRD core-host-node D-21).
//!
//! A node does the work on the machine it runs on and answers the core
//! through [`NodeLink`]. [`Local`] is the core's own machine, answered in the
//! same process.

use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cap_std::fs::Dir;
use hide_herdr_client::{ApiConnector, LocalSocketConnector};
use hide_host::serve::{Env, KitPlace};
use hide_node_link::RootIdentity;
use hide_node_link::protocol::Call;
use hide_node_link::{LinkAnswer, LinkError, NodeLink};

pub mod diagnostics;
pub mod opener;
pub mod pane_proof;
pub mod ssh;
pub mod terminal;

/// Plain calls this node works on at once. A call its caller stopped
/// waiting for keeps its thread until the work's own bound ends it, so this
/// caps those too; past it a call is refused as busy, nothing started
/// (engineering rule 15).
pub const MAX_IN_FLIGHT: usize = 64;

/// The machine this process runs on, answered in place, for the account
/// home it was given.
#[derive(Clone, Debug)]
pub struct Local {
    env: Env,
    in_flight: Arc<AtomicUsize>,
}

/// One plain call's place among [`MAX_IN_FLIGHT`], given back however its
/// work ends.
struct Admitted(Arc<AtomicUsize>);

impl Drop for Admitted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Local {
    /// The core's own node, answering for `home`: the home the core was
    /// configured with, which is not the process's for a daemon started with
    /// a private one. It installs no kit until [`Local::bundled`] says where
    /// the parts are.
    pub fn new(home: Option<PathBuf>) -> Self {
        Self {
            env: Env::standalone(home),
            in_flight: Arc::default(),
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
        Self {
            env,
            in_flight: Arc::default(),
        }
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
    /// Waits at most `timeout`, as a call to another machine does: the work
    /// runs on a thread of its own, and a caller that stops waiting reads
    /// the effect as unknown.
    fn call(&self, call: Call, timeout: Duration) -> Result<LinkAnswer, LinkError> {
        let op = op_name(&call);
        if self.in_flight.fetch_add(1, Ordering::AcqRel) >= MAX_IN_FLIGHT {
            self.in_flight.fetch_sub(1, Ordering::AcqRel);
            crate::diagnostic!(serde_json::json!({
                "component": "node",
                "kind": "node.busy",
                "op": op,
                "in_flight": MAX_IN_FLIGHT,
            }));
            return Err(LinkError::Busy);
        }
        let admitted = Admitted(Arc::clone(&self.in_flight));
        let env = self.env.clone();
        let (answered, answer) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("hide-node-call".to_owned())
            .spawn(move || {
                let _admitted = admitted;
                let _ = answered.send(hide_host::serve::handle_in(call, &env));
            })
            .map_err(|error| {
                LinkError::NotConnected(format!("This machine's node could not start: {error}"))
            })?;
        match answer.recv_timeout(timeout) {
            Ok(result) => result.map(LinkAnswer::Parsed).map_err(LinkError::Refused),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Its thread keeps a place until the work ends; these records
                // name what holds the places when the node turns busy.
                crate::diagnostic!(serde_json::json!({
                    "component": "node",
                    "kind": "node.call_unanswered",
                    "op": op,
                    "timeout_ms": timeout.as_millis() as u64,
                }));
                Err(LinkError::Unknown(format!(
                    "This machine's node did not answer within {} ms",
                    timeout.as_millis()
                )))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(LinkError::Unknown(
                "This machine's node ended the work without an answer".to_owned(),
            )),
        }
    }

    /// Runs in the caller's thread, which hears each report; once `timeout`
    /// has passed, the next report is answered with false, which stops the
    /// work.
    fn call_with_progress(
        &self,
        call: Call,
        timeout: Duration,
        progress: &mut dyn FnMut(serde_json::Value) -> bool,
    ) -> Result<LinkAnswer, LinkError> {
        let deadline = Instant::now().checked_add(timeout);
        let mut within =
            |report| deadline.is_none_or(|end| Instant::now() < end) && progress(report);
        hide_host::serve::handle_with_progress(call, &self.env, &mut within)
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

/// A request's name for a record (`save`, or `factory.verify_poll`), read
/// from the first bytes of its wire form, so a large request is never
/// written out whole.
fn op_name(call: &Call) -> String {
    const PREFIX: usize = 96;
    struct Prefix(Vec<u8>);
    impl std::io::Write for Prefix {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let room = PREFIX - self.0.len();
            if room == 0 {
                return Err(std::io::ErrorKind::WriteZero.into());
            }
            let taken = room.min(bytes.len());
            self.0.extend_from_slice(&bytes[..taken]);
            Ok(taken)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut prefix = Prefix(Vec::with_capacity(PREFIX));
    let _ = serde_json::to_writer(&mut prefix, call);
    let text = String::from_utf8_lossy(&prefix.0);
    let field = |name: &str| {
        let start = text.find(&format!("\"{name}\":\""))? + name.len() + 4;
        text[start..].split('"').next().map(str::to_owned)
    };
    match (field("op"), field("factory")) {
        (Some(op), Some(factory)) if op == "factory" => format!("factory.{factory}"),
        (Some(op), _) => op,
        (None, _) => "unknown".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record names the request without writing it out.
    #[test]
    fn a_request_is_named_from_its_first_bytes() {
        assert_eq!(op_name(&Call::Hello), "hello");
        let poll = Call::Factory {
            call: hide_node_link::factory::FactoryCall::VerifyPoll { id: "v".to_owned() },
        };
        assert_eq!(op_name(&poll), "factory.verify_poll");
        let large = Call::GitWatch {
            common_dirs: vec!["/".repeat(1 << 20)],
        };
        assert_eq!(op_name(&large), "git_watch");
    }

    /// A call waits at most its timeout, and past the cap of calls in
    /// flight another is refused as busy, nothing started.
    #[test]
    fn a_call_waits_at_most_its_timeout_and_past_the_cap_is_busy() {
        let node = Local::new(None);
        node.in_flight.store(MAX_IN_FLIGHT, Ordering::Release);
        assert!(matches!(
            node.call(Call::Hello, Duration::from_secs(5)),
            Err(LinkError::Busy)
        ));
        node.in_flight.store(0, Ordering::Release);
        let slow = Call::Factory {
            call: hide_node_link::factory::FactoryCall::Check {
                cwd: std::env::temp_dir().to_string_lossy().into_owned(),
                text: if cfg!(windows) {
                    "ping -n 3 127.0.0.1".to_owned()
                } else {
                    "sleep 2".to_owned()
                },
            },
        };
        let late = node.call(slow, Duration::from_millis(50));
        assert!(
            matches!(&late, Err(LinkError::Unknown(reason)) if reason.contains("did not answer")),
            "{late:?}"
        );
        assert!(node.call(Call::Hello, Duration::from_secs(5)).is_ok());
    }

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
