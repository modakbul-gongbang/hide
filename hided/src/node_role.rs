//! This daemon in the node role (PRD core-host-node-remote-core D-02, D-04,
//! D-08, D-09): the core runs on another machine, which the placement record
//! names, and this daemon keeps one link to it. It dials the core's machine
//! over SSH, runs the attach role there, trades the handshake, and answers
//! the core's calls on that channel exactly as a device the core dialed
//! answers them, with its own Herdr reached through the link.
//!
//! While the core cannot be reached the node waits and tries again, two
//! seconds after the first failure and doubling to a minute; a link that
//! lived a while starts the wait over. A watch thread looks every two
//! seconds for what makes the wait wrong (D-09): when the machine slept or
//! its network addresses changed, a waiting node tries at once and a live
//! link must answer an SSH ping within three seconds or is dropped and
//! dialed again; while the core's machine is unreachable, its SSH port is
//! probed and the node tries at once when the port answers again. Dropping
//! the role ends the link, the SSH connection and both threads.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use hide_node::ssh::SshAlias;
use hide_node::ssh::upstream::Upstream;
use hide_node::terminal::OutputSink;
use hide_node::terminal::device::NodeTerminals;
use hide_platform::ipc::{LocalStream, ShutdownHandle};
use serde_json::json;
use tokio::sync::watch;

use crate::attach::{self, Accepted, AttachOutcome, Line, NodeHello};
use crate::placement::Placement;

/// The first wait after a failed attempt.
const FIRST_WAIT: Duration = Duration::from_secs(2);
/// The longest wait between attempts.
const LONGEST_WAIT: Duration = Duration::from_secs(60);
/// A link that lived this long starts the wait over at [`FIRST_WAIT`].
const SETTLED: Duration = Duration::from_secs(30);
/// How long the core's machine has to answer the handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// How often the watch looks at the clocks, the network and, while the
/// core's machine is unreachable, its SSH port.
const WATCH_EVERY: Duration = Duration::from_secs(2);
/// The wall clock moving this much further than the monotonic clock, which
/// stops while the machine sleeps, between two looks means it slept.
const SLEPT: Duration = Duration::from_secs(5);
/// How long a live link's connection has to answer a ping after a wake.
const ANSWER_WITHIN: Duration = Duration::from_secs(3);
/// How long the core machine's SSH port has to take a probe's connection,
/// and then to greet it.
const PROBE_WITHIN: Duration = Duration::from_secs(1);

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
    /// its own panes' keys here, without the link.
    pub terminals: Arc<NodeTerminals>,
    /// The checkout roots the core opened on this node over the link: the
    /// node's screens read files under them without the core.
    pub roots: Arc<hide_host::serve::OpenedRoots>,
    /// The link's SSH connection, which a page of the core's machine
    /// reaches the core's loopback through (`node_pages`).
    pub upstream: Arc<Upstream>,
}

/// Where the node's link is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Phase {
    /// Dialing the core's machine and trading the handshake.
    Connecting,
    /// The core took the link.
    Live(Accepted),
    /// The last attempt failed or the link ended; the next one waits.
    Waiting { reason: LinkFailure },
}

/// Why the node's last attempt failed or its link ended. Its text is the
/// `core_link_reason` the node's health reports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkFailure {
    /// The core's machine could not be reached over SSH: the watch probes
    /// its SSH port and tries at once when it answers again.
    Unreachable(String),
    /// The core's machine answered, and its attach role reached no core.
    Attach(AttachOutcome),
    /// The core's machine answers as another core than the placement names.
    WrongCore(String),
    /// The core refused the node, with its reason (`other_build`,
    /// `own_node`, ...).
    Refused(String),
    /// Anything else on the way, or the link's end: its reason.
    Ended(String),
    /// The role is stopping.
    Stopping,
}

impl std::fmt::Display for LinkFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(error) => write!(formatter, "unreachable: {error}"),
            Self::Attach(outcome) => formatter.write_str(outcome.code()),
            Self::WrongCore(node) => write!(formatter, "wrong_core: the machine answers as {node}"),
            Self::Refused(reason) | Self::Ended(reason) => formatter.write_str(reason),
            Self::Stopping => formatter.write_str("stopping"),
        }
    }
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
    /// This machine's desktop windows' browser gateways, which the core
    /// asks over each link (`node_browser`).
    browser: Option<Arc<crate::node_browser::NodeBrowser>>,
}

/// The node role's link to its core, held for as long as the daemon runs.
pub struct NodeRole {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    watch: Option<JoinHandle<()>>,
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
        Self::start_with_browser(home, placement, identity, screen, None)
    }

    /// [`NodeRole::start`] whose links also answer the core for this
    /// machine's desktop windows (`browser`).
    pub fn start_with_browser(
        home: &Path,
        placement: Placement,
        identity: NodeIdentity,
        screen: Arc<dyn OutputSink>,
        browser: Option<Arc<crate::node_browser::NodeBrowser>>,
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
            browser,
        });
        let config = home.join(".ssh/config");
        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("node-role-link".to_owned())
                .spawn(move || keep_linked(&shared, &config, &placement, &identity))
                .map_err(|error| format!("the node link thread could not start: {error}"))?
        };
        let mut role = Self {
            shared,
            thread: Some(thread),
            watch: None,
        };
        // Dropping `role` on a failure here ends the link thread.
        let shared = Arc::clone(&role.shared);
        role.watch = Some(
            std::thread::Builder::new()
                .name("node-role-watch".to_owned())
                .spawn(move || watch(&shared))
                .map_err(|error| format!("the node watch thread could not start: {error}"))?,
        );
        Ok(role)
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
        wake(&self.shared);
    }
}

fn wake(shared: &Shared) {
    let mut state = lock(&shared.state);
    state.woken = true;
    if let Some(link) = state.link.take() {
        link.shutdown();
    }
    drop(state);
    // Closed outside the slot's lock: a close waits for a dial in progress,
    // which must not hold the next attempt or the role's end behind it.
    let upstream = lock(&shared.upstream).clone();
    if let Some(upstream) = upstream {
        upstream.close();
    }
    shared.changed.notify_all();
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
        let upstream = lock(&self.shared.upstream).take();
        if let Some(upstream) = upstream {
            upstream.close();
        }
        self.shared.changed.notify_all();
        for thread in [self.thread.take(), self.watch.take()]
            .into_iter()
            .flatten()
        {
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
        let reason = link_once(shared, config, placement, identity, generation);
        shared.live.send_replace(None);
        if started.elapsed() >= SETTLED {
            wait = FIRST_WAIT;
        }
        // The wait about to start: woken, it starts over, and otherwise the
        // one after it is twice as long.
        let this_wait = wait;
        herdr_core::diagnostic!(json!({
            "component": "node_role",
            "kind": "link.ended",
            "core": placement.node,
            "reason": reason.to_string(),
            "lived_ms": started.elapsed().as_millis() as u64,
            "retry_ms": wait.as_millis() as u64,
        }));
        if set_phase(shared, Phase::Waiting { reason }) {
            return;
        }
        let deadline = Instant::now() + this_wait;
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
        wait = next_wait(this_wait, std::mem::take(&mut state.woken));
    }
}

/// The wait after `wait`: a wake starts it over, and otherwise it doubles
/// up to [`LONGEST_WAIT`].
fn next_wait(wait: Duration, woken: bool) -> Duration {
    if woken {
        FIRST_WAIT
    } else {
        (wait * 2).min(LONGEST_WAIT)
    }
}

/// Looks every [`WATCH_EVERY`] for a sleep, a network change, or the core
/// machine's SSH port coming back, until the role stops.
fn watch(shared: &Shared) {
    let mut look = Look::new(SystemTime::now(), Instant::now(), network_addresses());
    let mut port = PortWatch::default();
    loop {
        {
            let state = lock(&shared.state);
            let (state, _) = shared
                .changed
                .wait_timeout_while(state, WATCH_EVERY, |state| !state.stopping)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.stopping {
                return;
            }
        }
        let moved = look.moved(SystemTime::now(), Instant::now(), network_addresses());
        let phase = lock(&shared.state).phase.clone();
        let upstream = lock(&shared.upstream).clone();
        if let Some(moved) = moved {
            port = PortWatch::default();
            let answered = match (&phase, &upstream) {
                (Phase::Live(_), Some(upstream)) => Some(upstream.alive(ANSWER_WITHIN)),
                _ => None,
            };
            herdr_core::diagnostic!(json!({
                "component": "node_role",
                "kind": "machine.moved",
                "moved": moved,
                "live_link_answered": answered,
            }));
            if answered != Some(true) && !matches!(phase, Phase::Connecting) {
                wake(shared);
            }
            continue;
        }
        match &upstream {
            Some(upstream) if probes_port(&phase, &port) => {
                if port.came_back(upstream.reachable(PROBE_WITHIN)) {
                    herdr_core::diagnostic!(json!({
                        "component": "node_role",
                        "kind": "core.reachable_again",
                    }));
                    wake(shared);
                }
            }
            _ => port = PortWatch::default(),
        }
    }
}

/// Whether the watch probes the core machine's SSH port: only while the
/// node waits after a dial that could not reach the machine, and only until
/// the port answers. Past that the dial failed on something else (a key, a
/// host key, the account), and the wait's own retry tries it again.
fn probes_port(phase: &Phase, port: &PortWatch) -> bool {
    matches!(
        phase,
        Phase::Waiting {
            reason: LinkFailure::Unreachable(_)
        }
    ) && port.last != Some(true)
}

fn network_addresses() -> Option<BTreeSet<IpAddr>> {
    hide_platform::host::network_addresses().ok()
}

/// What the watch saw at its last look.
struct Look {
    wall: SystemTime,
    monotonic: Instant,
    addresses: Option<BTreeSet<IpAddr>>,
}

impl Look {
    fn new(wall: SystemTime, monotonic: Instant, addresses: Option<BTreeSet<IpAddr>>) -> Self {
        Self {
            wall,
            monotonic,
            addresses,
        }
    }

    /// Takes a new look, and answers what moved since the last one:
    /// `"slept"` when the wall clock ran [`SLEPT`] past the monotonic one,
    /// `"network"` when the address set changed. An address set that could
    /// not be read says nothing.
    fn moved(
        &mut self,
        wall: SystemTime,
        monotonic: Instant,
        addresses: Option<BTreeSet<IpAddr>>,
    ) -> Option<&'static str> {
        let walked = wall.duration_since(self.wall).unwrap_or_default();
        let ran = monotonic.saturating_duration_since(self.monotonic);
        let slept = walked.saturating_sub(ran) >= SLEPT;
        let network = match (&self.addresses, &addresses) {
            (Some(before), Some(now)) => before != now,
            _ => false,
        };
        self.wall = wall;
        self.monotonic = monotonic;
        if addresses.is_some() {
            self.addresses = addresses;
        }
        if slept {
            Some("slept")
        } else if network {
            Some("network")
        } else {
            None
        }
    }
}

/// The core machine's SSH port across the probes of one unreachable wait.
#[derive(Default)]
struct PortWatch {
    last: Option<bool>,
}

impl PortWatch {
    /// Records a probe; `true` when the port answers after a probe it did
    /// not. A port that answers from the first probe on says nothing: the
    /// dial failed on something a new attempt at once would not change.
    fn came_back(&mut self, answers: bool) -> bool {
        let came_back = self.last == Some(false) && answers;
        self.last = Some(answers);
        came_back
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
) -> LinkFailure {
    match try_link(shared, config, placement, identity, generation) {
        Ok(ended) | Err(ended) => ended,
    }
}

fn try_link(
    shared: &Shared,
    config: &Path,
    placement: &Placement,
    identity: &NodeIdentity,
    generation: u64,
) -> Result<LinkFailure, LinkFailure> {
    // The alias is read again for every attempt, so a fix the operator
    // makes to it reaches the next dial; a connection to an alias that
    // changed is ended and a new one made.
    let alias = SshAlias::from_config_file(config, &placement.alias)
        .map_err(|error| LinkFailure::Ended(format!("ssh_alias: {}", error.diagnostic().reason)))?;
    let upstream = {
        let mut slot = lock(&shared.upstream);
        match slot.as_ref() {
            Some(upstream) if *upstream.alias() == alias => Arc::clone(upstream),
            _ => {
                let upstream = Arc::new(
                    Upstream::new(alias)
                        .map_err(|error| LinkFailure::Ended(format!("ssh: {error}")))?,
                );
                let replaced = slot.replace(Arc::clone(&upstream));
                drop(slot);
                if let Some(replaced) = replaced {
                    replaced.close();
                }
                upstream
            }
        }
    };
    let channel = upstream
        .attach(&placement.program, placement.state_dir.as_deref())
        .map_err(|error| LinkFailure::Unreachable(error.to_string()))?;
    let stream = channel.stream;
    {
        let mut state = lock(&shared.state);
        if state.stopping {
            return Err(LinkFailure::Stopping);
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
    upstream: &Arc<Upstream>,
    generation: u64,
) -> Result<LinkFailure, LinkFailure> {
    let ended = LinkFailure::Ended;
    let mut reader = BufReader::new(stream.duplicate());
    let mut writer = stream.duplicate();
    reader
        .get_ref()
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(|error| ended(error.to_string()))?;
    let silent = |message: String| {
        let said = String::from_utf8_lossy(&lock(stderr)).trim().to_owned();
        if said.is_empty() {
            ended(message)
        } else {
            ended(format!("{message} ({said})"))
        }
    };
    match attach::read_line(&mut reader).map_err(silent)? {
        Line::Attach(outcome) => return Err(LinkFailure::Attach(outcome)),
        Line::Core(core) if core.node != placement.node => {
            return Err(LinkFailure::WrongCore(core.node));
        }
        Line::Core(_) => {}
        _ => {
            return Err(ended(
                "the core's machine answered another line than its core's".to_owned(),
            ));
        }
    }
    attach::write_line(
        &mut writer,
        &Line::Node(NodeHello {
            node: identity.node.clone(),
            label: identity.label.clone(),
            build: identity.build.clone(),
            herdr_socket: identity.herdr_socket.display().to_string(),
        }),
    )
    .map_err(ended)?;
    let accepted = match attach::read_line(&mut reader).map_err(ended)? {
        Line::Accepted(accepted) => accepted,
        Line::Refused(refusal) => return Err(LinkFailure::Refused(refusal.reason)),
        _ => {
            return Err(ended(
                "the core answered another line than its answer".to_owned(),
            ));
        }
    };
    reader
        .get_ref()
        .set_read_timeout(None)
        .map_err(|error| ended(error.to_string()))?;
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
        .map_err(|error| ended(format!("relay_forward: {error}")))?;
    let terminals = Arc::new(NodeTerminals::for_screen(
        Arc::clone(&shared.screen),
        identity.herdr_bin.clone(),
    ));
    let roots = Arc::new(hide_host::serve::OpenedRoots::default());
    if set_phase(shared, Phase::Live(accepted.clone())) {
        return Err(LinkFailure::Stopping);
    }
    shared.live.send_replace(Some(Arc::new(LiveLink {
        generation,
        accepted,
        relay_port: forward.port(),
        terminals: Arc::clone(&terminals),
        roots: Arc::clone(&roots),
        upstream: Arc::clone(upstream),
    })));
    let browser = shared
        .browser
        .as_ref()
        .map(|browser| crate::node_browser::LinkBrowser {
            browser: Arc::clone(browser),
            generation,
        });
    let link_end = serve(
        reader,
        writer,
        identity,
        &terminals,
        &roots,
        browser
            .as_ref()
            .map(|browser| browser as &dyn hide_host::link_bridge::BrowserGateway),
    );
    shared.live.send_replace(None);
    drop(forward);
    Ok(LinkFailure::Ended(match link_end {
        Ok(reason) | Err(reason) => reason,
    }))
}

/// Answers the core's calls until the link ends. The reader keeps what it
/// buffered past the handshake: the core's first call.
fn serve(
    reader: impl BufRead,
    writer: LocalStream,
    identity: &NodeIdentity,
    terminals: &NodeTerminals,
    roots: &hide_host::serve::OpenedRoots,
    browser: Option<&dyn hide_host::link_bridge::BrowserGateway>,
) -> Result<String, String> {
    hide_host::serve::serve_with(
        reader,
        writer,
        hide_host::serve::Services {
            terminals: Some(terminals),
            herdr_socket: Some(identity.herdr_socket.clone()),
            heartbeat: true,
            checkout_callers: true,
            opened_roots: Some(roots),
            browser,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wait_doubles_to_a_minute_and_a_wake_starts_it_over() {
        let mut wait = FIRST_WAIT;
        let mut waits = Vec::new();
        for _ in 0..7 {
            waits.push(wait.as_secs());
            wait = next_wait(wait, false);
        }
        assert_eq!(waits, [2, 4, 8, 16, 32, 60, 60]);
        assert_eq!(next_wait(LONGEST_WAIT, true), FIRST_WAIT);
    }

    fn addresses(last: u8) -> Option<BTreeSet<IpAddr>> {
        Some(BTreeSet::from([IpAddr::from([192, 168, 1, last])]))
    }

    #[test]
    fn a_sleep_or_a_new_address_set_is_seen_and_an_ordinary_look_is_not() {
        let wall = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let monotonic = Instant::now();
        let mut look = Look::new(wall, monotonic, addresses(2));
        let step = WATCH_EVERY;
        assert_eq!(
            look.moved(wall + step, monotonic + step, addresses(2)),
            None
        );
        // The monotonic clock stood still for a minute the wall clock ran.
        let wall = wall + step + Duration::from_secs(60);
        let monotonic = monotonic + step * 2;
        assert_eq!(look.moved(wall, monotonic, addresses(2)), Some("slept"));
        let (wall, monotonic) = (wall + step, monotonic + step);
        assert_eq!(look.moved(wall, monotonic, addresses(3)), Some("network"));
        // An unreadable set says nothing and keeps the last one.
        let (wall, monotonic) = (wall + step, monotonic + step);
        assert_eq!(look.moved(wall, monotonic, None), None);
        let (wall, monotonic) = (wall + step, monotonic + step);
        assert_eq!(look.moved(wall, monotonic, addresses(3)), None);
        // A wall clock set back is not a sleep.
        assert_eq!(
            look.moved(
                wall - Duration::from_secs(3600),
                monotonic + step,
                addresses(3)
            ),
            None
        );
    }

    /// The port is probed for a dial that could not reach the core's
    /// machine, whatever its text, and never for another failure whose text
    /// happens to read the same.
    #[test]
    fn only_an_unreachable_dial_probes_the_core_machine_s_port() {
        let waiting = |reason| Phase::Waiting { reason };
        let silent = PortWatch::default();
        assert!(probes_port(
            &waiting(LinkFailure::Unreachable("connection refused".to_owned())),
            &silent
        ));
        for reason in [
            LinkFailure::Refused("unreachable_by_policy".to_owned()),
            LinkFailure::Ended("unreachable: the link dropped".to_owned()),
            LinkFailure::Attach(AttachOutcome::NoCore),
            LinkFailure::Stopping,
        ] {
            assert!(
                !probes_port(&waiting(reason.clone()), &silent),
                "{reason:?}"
            );
        }
        let answered = PortWatch { last: Some(true) };
        assert!(!probes_port(
            &waiting(LinkFailure::Unreachable("connection refused".to_owned())),
            &answered
        ));
    }

    #[test]
    fn only_a_port_that_was_silent_and_answers_again_wakes_the_node() {
        let mut port = PortWatch::default();
        assert!(!port.came_back(true), "a port answering from the start");
        assert!(!port.came_back(true));
        let mut port = PortWatch::default();
        assert!(!port.came_back(false));
        assert!(!port.came_back(false));
        assert!(port.came_back(true));
        assert!(!port.came_back(true), "once per return");
    }
}
