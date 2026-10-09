//! This daemon in the node role (PRD core-host-node-remote-core D-02, D-04,
//! D-08, D-09): the core runs on another machine, which the placement record
//! names, and this daemon keeps one link to it. It dials the core's machine
//! over SSH, runs the attach role there, trades the handshake, and answers
//! the core's calls on that channel exactly as a device the core dialed
//! answers them, with its own Herdr reached through the link.
//!
//! While the core cannot be reached the node waits and tries again, two
//! seconds after the first failure and doubling to a minute; a link that
//! lived a while starts the wait over, and [`NodeRole::wake`] (the machine
//! woke or its network changed) tries at once. Dropping the role ends the
//! link, the SSH connection and the thread that holds them.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hide_node::ssh::SshAlias;
use hide_node::ssh::upstream::Upstream;
use hide_node::terminal::OutputSink;
use hide_node::terminal::device::NodeTerminals;
use hide_platform::ipc::{LocalStream, ShutdownHandle};
use serde_json::json;
use tokio::sync::watch;

use crate::attach::{self, Accepted, Line, NodeHello};
use crate::placement::Placement;

/// The first wait after a failed attempt.
const FIRST_WAIT: Duration = Duration::from_secs(2);
/// The longest wait between attempts.
const LONGEST_WAIT: Duration = Duration::from_secs(60);
/// A link that lived this long starts the wait over at [`FIRST_WAIT`].
const SETTLED: Duration = Duration::from_secs(30);
/// How long the core's machine has to answer the handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Who this node is, as it tells its core.
#[derive(Clone, Debug)]
pub struct NodeIdentity {
    pub node: String,
    pub label: String,
    pub build: String,
    /// This machine's own Herdr, which the core reaches only through the
    /// link.
    pub herdr_socket: PathBuf,
    /// The Herdr binary this machine's panes attach with.
    pub herdr_bin: PathBuf,
}

/// A link the core took, as the node's screens reach the core through it.
pub struct LiveLink {
    /// Counts links since the role started: a screen that opened on one
    /// link ends with it.
    pub generation: u64,
    pub accepted: Accepted,
    /// The loopback port on this machine whose connections reach the
    /// core's port through the link's SSH connection.
    pub relay_port: u16,
    /// This node's terminals for the link's life: the node's screens send
    /// its own panes' keys and views here, without the link.
    pub terminals: Arc<NodeTerminals>,
}

/// Where the node's link is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Phase {
    /// Dialing the core's machine and trading the handshake.
    Connecting,
    /// The core took the link.
    Live(Accepted),
    /// The last attempt failed or the link ended; the next one waits.
    Waiting { reason: String },
}

struct State {
    phase: Phase,
    stopping: bool,
    /// Set by [`NodeRole::wake`]: the next attempt does not wait.
    woken: bool,
    /// The live link's stream, ended to end the link.
    link: Option<ShutdownHandle>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    upstream: Mutex<Option<Arc<Upstream>>>,
    live: watch::Sender<Option<Arc<LiveLink>>>,
    /// Where the node's own panes' output goes besides the link.
    screen: Arc<dyn OutputSink>,
}

/// The node role's link to its core, held for as long as the daemon runs.
pub struct NodeRole {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl NodeRole {
    /// Starts keeping the link to the core `placement` names; `home` is
    /// the account whose `~/.ssh/config` names its alias.
    /// Its own panes' output reaches `screen`, the node's screens.
    pub fn start(
        home: &Path,
        placement: Placement,
        identity: NodeIdentity,
        screen: Arc<dyn OutputSink>,
    ) -> Result<Self, String> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                phase: Phase::Connecting,
                stopping: false,
                woken: false,
                link: None,
            }),
            changed: Condvar::new(),
            upstream: Mutex::new(None),
            live: watch::Sender::new(None),
            screen,
        });
        let config = home.join(".ssh/config");
        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("node-role-link".to_owned())
                .spawn(move || keep_linked(&shared, &config, &placement, &identity))
                .map_err(|error| format!("the node link thread could not start: {error}"))?
        };
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    pub fn phase(&self) -> Phase {
        lock(&self.shared.state).phase.clone()
    }

    /// The live link, as it changes: `None` while there is none.
    pub fn live(&self) -> watch::Receiver<Option<Arc<LiveLink>>> {
        self.shared.live.subscribe()
    }

    /// Waits up to `timeout` for the phase to satisfy `done`, and answers
    /// the phase it ended on.
    pub fn wait_for(&self, timeout: Duration, done: impl Fn(&Phase) -> bool) -> Phase {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.shared.state);
        while !done(&state.phase) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
        state.phase.clone()
    }

    /// The machine woke or its network changed: a link that may be dead is
    /// dropped and the next attempt goes at once.
    pub fn wake(&self) {
        let mut state = lock(&self.shared.state);
        state.woken = true;
        if let Some(link) = state.link.take() {
            link.shutdown();
        }
        drop(state);
        if let Some(upstream) = lock(&self.shared.upstream).as_ref() {
            upstream.close();
        }
        self.shared.changed.notify_all();
    }
}

impl Drop for NodeRole {
    fn drop(&mut self) {
        {
            let mut state = lock(&self.shared.state);
            state.stopping = true;
            if let Some(link) = state.link.take() {
                link.shutdown();
            }
        }
        if let Some(upstream) = lock(&self.shared.upstream).take() {
            upstream.close();
        }
        self.shared.changed.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn keep_linked(shared: &Shared, config: &Path, placement: &Placement, identity: &NodeIdentity) {
    let mut wait = FIRST_WAIT;
    let mut generation = 0;
    loop {
        generation += 1;
        if set_phase(shared, Phase::Connecting) {
            return;
        }
        let started = Instant::now();
        let ended = link_once(shared, config, placement, identity, generation);
        shared.live.send_replace(None);
        let reason = match ended {
            Ok(reason) | Err(reason) => reason,
        };
        if started.elapsed() >= SETTLED {
            wait = FIRST_WAIT;
        }
        herdr_core::diagnostic!(json!({
            "component": "node_role",
            "kind": "link.ended",
            "core": placement.node,
            "reason": reason,
            "lived_ms": started.elapsed().as_millis() as u64,
            "retry_ms": wait.as_millis() as u64,
        }));
        if set_phase(shared, Phase::Waiting { reason }) {
            return;
        }
        let deadline = Instant::now() + wait;
        let mut state = lock(&shared.state);
        while !state.stopping && !state.woken {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            state = shared
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
        if state.stopping {
            return;
        }
        if std::mem::take(&mut state.woken) {
            wait = FIRST_WAIT;
        } else {
            wait = (wait * 2).min(LONGEST_WAIT);
        }
    }
}

/// Sets the phase; `true` when the role is stopping.
fn set_phase(shared: &Shared, phase: Phase) -> bool {
    let mut state = lock(&shared.state);
    if state.phase != phase {
        state.phase = phase;
        shared.changed.notify_all();
    }
    state.stopping
}

/// One link: the handshake, then the core's calls until the link ends.
/// Answers why it ended.
fn link_once(
    shared: &Shared,
    config: &Path,
    placement: &Placement,
    identity: &NodeIdentity,
    generation: u64,
) -> Result<String, String> {
    let upstream = {
        let mut slot = lock(&shared.upstream);
        match slot.as_ref() {
            Some(upstream) => Arc::clone(upstream),
            None => {
                let alias = SshAlias::from_config_file(config, &placement.alias)
                    .map_err(|error| format!("ssh_alias: {}", error.diagnostic().reason))?;
                let upstream =
                    Arc::new(Upstream::new(alias).map_err(|error| format!("ssh: {error}"))?);
                *slot = Some(Arc::clone(&upstream));
                upstream
            }
        }
    };
    let channel = upstream
        .attach(&placement.program, placement.state_dir.as_deref())
        .map_err(|error| format!("unreachable: {error}"))?;
    let stream = channel.stream;
    {
        let mut state = lock(&shared.state);
        if state.stopping {
            return Err("stopping".to_owned());
        }
        state.link = Some(stream.shutdown_handle());
    }
    let ended = serve_link(
        &stream,
        placement,
        identity,
        shared,
        &channel.stderr,
        &upstream,
        generation,
    );
    lock(&shared.state).link = None;
    stream.shutdown_handle().shutdown();
    ended
}

fn serve_link(
    stream: &LocalStream,
    placement: &Placement,
    identity: &NodeIdentity,
    shared: &Shared,
    stderr: &Mutex<Vec<u8>>,
    upstream: &Upstream,
    generation: u64,
) -> Result<String, String> {
    let mut reader = BufReader::new(stream.duplicate());
    let mut writer = stream.duplicate();
    reader
        .get_ref()
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let silent = |message: String| {
        let said = String::from_utf8_lossy(&lock(stderr)).trim().to_owned();
        if said.is_empty() {
            message
        } else {
            format!("{message} ({said})")
        }
    };
    match attach::read_line(&mut reader).map_err(silent)? {
        Line::Attach(answer) if answer == attach::NO_CORE => return Err("no_core".to_owned()),
        Line::Core(core) if core.node != placement.node => {
            return Err(format!("wrong_core: the machine answers as {}", core.node));
        }
        Line::Core(_) => {}
        _ => return Err("the core's machine answered another line than its core's".to_owned()),
    }
    attach::write_line(
        &mut writer,
        &Line::Node(NodeHello {
            node: identity.node.clone(),
            label: identity.label.clone(),
            build: identity.build.clone(),
            herdr_socket: identity.herdr_socket.display().to_string(),
        }),
    )?;
    let accepted = match attach::read_line(&mut reader)? {
        Line::Accepted(accepted) => accepted,
        Line::Refused(refusal) => return Err(refusal.reason),
        _ => return Err("the core answered another line than its answer".to_owned()),
    };
    reader
        .get_ref()
        .set_read_timeout(None)
        .map_err(|error| error.to_string())?;
    herdr_core::diagnostic!(json!({
        "component": "node_role",
        "kind": "link.accepted",
        "core": placement.node,
        "port": accepted.port,
    }));
    // The screens' way to the core: held for the link's life, and ended
    // with it.
    let forward = upstream
        .forward(accepted.port)
        .map_err(|error| format!("relay_forward: {error}"))?;
    let terminals = Arc::new(NodeTerminals::for_screen(
        Arc::clone(&shared.screen),
        identity.herdr_bin.clone(),
    ));
    if set_phase(shared, Phase::Live(accepted.clone())) {
        return Err("stopping".to_owned());
    }
    shared.live.send_replace(Some(Arc::new(LiveLink {
        generation,
        accepted,
        relay_port: forward.port(),
        terminals: Arc::clone(&terminals),
    })));
    let ended = serve(reader, writer, identity, &terminals);
    shared.live.send_replace(None);
    drop(forward);
    ended
}

/// Answers the core's calls until the link ends. The reader keeps what it
/// buffered past the handshake: the core's first call.
fn serve(
    reader: impl BufRead,
    writer: LocalStream,
    identity: &NodeIdentity,
    terminals: &NodeTerminals,
) -> Result<String, String> {
    hide_host::serve::serve_with(
        reader,
        writer,
        hide_host::serve::Services {
            terminals: Some(terminals),
            herdr_socket: Some(identity.herdr_socket.clone()),
            heartbeat: true,
        },
    )
    .map(|()| "link_closed".to_owned())
    .map_err(|error| format!("link_failed: {error}"))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
