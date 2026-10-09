//! One label generator per Herdr server (PRD labels-in-hided D-10,
//! core-host-node-remote-core amendment 3).
//!
//! Every core that follows the same Herdr server would otherwise analyze the
//! same turns. The worker whose node holds the server's lock for it
//! generates; any other shows provider names, logs once that it is standing
//! by, and asks again every thirty seconds, so it takes over when the holder
//! ends. The lock is the node's (`hide_host::label_lock`): it sits beside
//! the server's socket on the machine that runs the server, so cores on any
//! machine that label one server meet at one file, and the core keeps no
//! file of its own for it.
//!
//! A worker that generates asks again every thirty seconds too: a node lets
//! its locks go with the link it served, so a link that came back has to
//! take the lock again, or learn that another core took it meanwhile. The
//! core's own node answers in this process, at once; a linked node's answer
//! crosses its link, so that ask runs on a thread of its own, because the
//! worker runs on the session-sync thread, which never blocks. A node that
//! serves no Herdr server it can name has no shared place to meet at, and
//! its worker generates.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hide_node_link::ErrorCode;
use hide_node_link::protocol::{Call, LabelLock};
use serde_json::json;

use super::ChannelSource;
use super::worker::Wake;
use crate::node_access::{LinkError, NodeLink, call_as};

const RETRY: Duration = Duration::from_secs(30);
/// How long one ask of a linked node may take; its thread ends with it.
const ASK_WITHIN: Duration = Duration::from_secs(10);

/// Names each worker to its node, so a worker that replaces another never
/// shares or frees the lock the other took.
static NEXT_GENERATOR: AtomicU64 = AtomicU64::new(1);

/// Where a worker's lock is: the node of the machine that runs its Herdr
/// server, and the server's socket there when the core knows it; `None`
/// asks for the server the node's pane service serves.
pub(crate) struct LockPlace {
    pub(crate) node: ChannelSource,
    pub(crate) herdr_socket: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Role {
    /// No answer yet.
    Unasked,
    /// Another core generates for the server.
    Standby,
    /// The node holds the lock for this worker.
    Held,
    /// The node names no server to lock, so nothing is shared.
    Unshared,
}

impl Role {
    fn generates(self) -> bool {
        matches!(self, Self::Held | Self::Unshared)
    }
}

enum Answer {
    Lock(LabelLock),
    NoServer,
    Failed(String),
}

/// A linked node's ask, shared with the thread that runs it.
#[derive(Default)]
struct Asked {
    answer: Option<Answer>,
    /// The worker ended; a lock the ask took is given back by its thread.
    ended: bool,
}

pub(crate) struct GeneratorLock {
    place: LockPlace,
    target: String,
    generator: u64,
    wake: Wake,
    role: Role,
    /// The node the last ask went to, which the lock is given back to.
    node: Option<Arc<dyn NodeLink>>,
    asking: Option<Arc<Mutex<Asked>>>,
    next_attempt: Option<Instant>,
    /// What was last logged, so a state is logged once, not every ask.
    logged: Option<&'static str>,
}

impl GeneratorLock {
    pub(crate) fn new(place: LockPlace, target: &str, wake: Wake) -> Self {
        Self {
            place,
            target: target.to_owned(),
            generator: NEXT_GENERATOR.fetch_add(1, Ordering::Relaxed),
            wake,
            role: Role::Unasked,
            node: None,
            asking: None,
            next_attempt: None,
            logged: None,
        }
    }

    /// Takes the answer that came and asks again when it is due. Returns
    /// `(changed, took_over)`: whether this worker started or stopped
    /// generating, and whether it was standing by until now, so what changed
    /// while another core generated has to be caught up.
    pub(crate) fn ensure(&mut self, now: Instant) -> (bool, bool) {
        let before = self.role;
        if let Some(answer) = self.take_answer() {
            self.apply(answer);
        }
        if self.asking.is_none() && self.next_attempt.is_none_or(|at| now >= at) {
            self.next_attempt = Some(now + RETRY);
            if let Some(answer) = self.ask() {
                self.apply(answer);
            }
        }
        (
            before.generates() != self.role.generates(),
            before == Role::Standby && self.role.generates(),
        )
    }

    pub(crate) fn held(&self) -> bool {
        self.role.generates()
    }

    fn call(&self, take: bool) -> Call {
        let herdr_socket = self.place.herdr_socket.clone();
        let generator = self.generator;
        if take {
            Call::LabelLock {
                herdr_socket,
                generator,
            }
        } else {
            Call::LabelUnlock {
                herdr_socket,
                generator,
            }
        }
    }

    /// Asks the node: in this process, the answer; across a link, `None`,
    /// and the answer comes on a later `ensure` after the worker is woken.
    fn ask(&mut self) -> Option<Answer> {
        let node = match (self.place.node)() {
            Ok(node) => node,
            Err(reason) => return Some(Answer::Failed(reason.to_owned())),
        };
        self.node = Some(Arc::clone(&node));
        let call = self.call(true);
        if node.in_process() {
            return Some(ask(node.as_ref(), call));
        }
        let asked = Arc::new(Mutex::new(Asked::default()));
        let shared = Arc::clone(&asked);
        let wake = Arc::clone(&self.wake);
        let unlock = self.call(false);
        let spawned = std::thread::Builder::new()
            .name("label-lock".to_owned())
            .spawn(move || {
                let answer = ask(node.as_ref(), call);
                let mut asked = shared.lock().unwrap_or_else(|error| error.into_inner());
                if asked.ended {
                    drop(asked);
                    if matches!(answer, Answer::Lock(LabelLock { held: true, .. })) {
                        give_back(node.as_ref(), unlock);
                    }
                    return;
                }
                asked.answer = Some(answer);
                drop(asked);
                wake();
            });
        match spawned {
            Ok(_) => {
                self.asking = Some(asked);
                None
            }
            Err(error) => Some(Answer::Failed(error.to_string())),
        }
    }

    fn take_answer(&mut self) -> Option<Answer> {
        let answer = self
            .asking
            .as_ref()?
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .answer
            .take()?;
        self.asking = None;
        Some(answer)
    }

    fn apply(&mut self, answer: Answer) {
        let (role, kind, holder, message) = match answer {
            Answer::Lock(LabelLock { held: true, .. }) => {
                let kind = (self.role == Role::Standby).then_some("generator.acquired");
                (Role::Held, kind, None, None)
            }
            Answer::Lock(LabelLock {
                held: false,
                holder,
            }) => (Role::Standby, Some("generator.standby"), holder, None),
            Answer::NoServer => (Role::Unshared, Some("generator.unshared"), None, None),
            // The link is down or the node failed: this worker keeps what it
            // had until a node answers, and reads nothing meanwhile, since
            // its reads cross the same link.
            Answer::Failed(message) => (
                self.role,
                Some("generator.lock_failed"),
                None,
                Some(message),
            ),
        };
        self.role = role;
        if let Some(kind) = kind
            && self.logged != Some(kind)
        {
            self.logged = Some(kind);
            crate::diagnostic!(json!({
                "component": "labels",
                "kind": kind,
                "target": self.target,
                "holder": holder,
                "message": message,
            }));
        }
    }
}

impl Drop for GeneratorLock {
    fn drop(&mut self) {
        let mut held = self.role == Role::Held;
        if let Some(asked) = self.asking.take() {
            let mut asked = asked.lock().unwrap_or_else(|error| error.into_inner());
            asked.ended = true;
            held |= matches!(
                asked.answer,
                Some(Answer::Lock(LabelLock { held: true, .. }))
            );
        }
        let Some(node) = self.node.take().filter(|_| held) else {
            return;
        };
        let unlock = self.call(false);
        if node.in_process() {
            give_back(node.as_ref(), unlock);
            return;
        }
        // Across a link the give-back runs on a thread of its own, ended by
        // the ask's deadline; a link that is gone has let the lock go.
        let target = self.target.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("label-unlock".to_owned())
            .spawn(move || give_back(node.as_ref(), unlock))
        {
            crate::diagnostic!(json!({
                "component": "labels",
                "kind": "generator.release_failed",
                "target": target,
                "message": error.to_string(),
            }));
        }
    }
}

fn ask(node: &dyn NodeLink, call: Call) -> Answer {
    match call_as::<LabelLock>(node, call, ASK_WITHIN) {
        Ok(lock) => Answer::Lock(lock),
        // A node that names no server, or one that predates the call, has
        // no shared place to meet at.
        Err(LinkError::Refused(error))
            if matches!(
                error.code,
                ErrorCode::Unsupported | ErrorCode::InvalidRequest
            ) =>
        {
            Answer::NoServer
        }
        Err(error) => Answer::Failed(error.to_string()),
    }
}

fn give_back(node: &dyn NodeLink, unlock: Call) {
    if let Err(error) = call_as::<()>(node, unlock, ASK_WITHIN) {
        crate::diagnostic!(json!({
            "component": "labels",
            "kind": "generator.release_failed",
            "message": error.to_string(),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A worker whose node is `node`, for the server at `socket`.
    fn lock(node: &Arc<dyn NodeLink>, socket: &std::path::Path) -> GeneratorLock {
        let node = Arc::clone(node);
        GeneratorLock::new(
            LockPlace {
                node: Box::new(move || Ok(Arc::clone(&node))),
                herdr_socket: Some(socket.display().to_string()),
            },
            "local",
            Arc::new(|| {}),
        )
    }

    fn local() -> Arc<dyn NodeLink> {
        Arc::new(hide_node::Local::new(None))
    }

    #[test]
    fn a_second_core_for_the_same_server_stands_by_until_the_first_ends() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("herdr.sock");
        let (one, two) = (local(), local());
        let mut first = lock(&one, &socket);
        let mut second = lock(&two, &socket);
        let mut other = lock(&two, &dir.path().join("other.sock"));
        let now = Instant::now();
        assert_eq!(first.ensure(now), (true, false));
        assert_eq!(second.ensure(now), (false, false));
        assert!(!second.held());
        assert_eq!(other.ensure(now), (true, false));
        drop(first);
        // Inside the retry window nothing is asked; after it, it takes over.
        assert_eq!(second.ensure(now + Duration::from_secs(1)), (false, false));
        assert_eq!(second.ensure(now + RETRY), (true, true));
    }

    #[test]
    fn a_worker_that_replaces_another_on_one_node_waits_for_its_lock() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("herdr.sock");
        let node = local();
        let mut old = lock(&node, &socket);
        let mut new = lock(&node, &socket);
        let now = Instant::now();
        assert_eq!(old.ensure(now), (true, false));
        assert_eq!(new.ensure(now), (false, false));
        drop(old);
        assert_eq!(new.ensure(now + RETRY), (true, true));
    }

    /// A node across a link, answering a lock on the asking thread only
    /// after the test lets it, and saying each give-back it answered.
    struct Linked {
        node: Arc<dyn NodeLink>,
        gate: Mutex<std::sync::mpsc::Receiver<()>>,
        given_back: Mutex<std::sync::mpsc::Sender<()>>,
    }

    impl NodeLink for Linked {
        fn call(
            &self,
            call: Call,
            timeout: Duration,
        ) -> Result<hide_node_link::LinkAnswer, LinkError> {
            let unlock = matches!(call, Call::LabelUnlock { .. });
            if !unlock {
                let _ = self.gate.lock().unwrap().recv();
            }
            let answer = self.node.call(call, timeout);
            if unlock {
                let _ = self.given_back.lock().unwrap().send(());
            }
            answer
        }
    }

    #[test]
    fn a_linked_nodes_answer_comes_without_blocking_and_a_late_lock_is_given_back() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("herdr.sock");
        let (open, gate) = std::sync::mpsc::channel();
        let (gave, given_back) = std::sync::mpsc::channel();
        let linked = Arc::new(Linked {
            node: local(),
            gate: Mutex::new(gate),
            given_back: Mutex::new(gave),
        });
        let (woke, woken) = std::sync::mpsc::channel();
        let woke = Mutex::new(woke);
        let place = || {
            let linked: Arc<dyn NodeLink> = linked.clone();
            LockPlace {
                node: Box::new(move || Ok(Arc::clone(&linked))),
                herdr_socket: Some(socket.display().to_string()),
            }
        };
        let mut worker = GeneratorLock::new(
            place(),
            "device:mini",
            Arc::new(move || {
                let _ = woke.lock().unwrap().send(());
            }),
        );
        let now = Instant::now();
        assert_eq!(worker.ensure(now), (false, false), "the ask is out");
        open.send(()).unwrap();
        woken.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(worker.ensure(now), (true, false));
        assert_eq!(lock(&local(), &socket).ensure(now), (false, false));
        drop(worker);
        given_back.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(lock(&local(), &socket).ensure(now), (true, false));

        // A worker that ends while its ask is out gives back what the ask
        // takes after it ended.
        let mut late = GeneratorLock::new(place(), "device:mini", Arc::new(|| {}));
        assert_eq!(late.ensure(now), (false, false));
        drop(late);
        open.send(()).unwrap();
        given_back.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(lock(&local(), &socket).ensure(now), (true, false));
    }

    #[test]
    fn a_node_that_names_no_server_leaves_its_worker_generating() {
        let node: Arc<dyn NodeLink> = local();
        let mut worker = GeneratorLock::new(
            LockPlace {
                node: Box::new(move || Ok(Arc::clone(&node))),
                herdr_socket: None,
            },
            "device:mini",
            Arc::new(|| {}),
        );
        assert_eq!(worker.ensure(Instant::now()), (true, false));
    }
}
