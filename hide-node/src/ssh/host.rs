//! A device's node: `hided node serve` answering the `hide_host` contract
//! over one SSH exec channel (PRD S5.5 D-05, D-20, D-23).
//!
//! Nothing here runs without the operator's consent for that device, recorded
//! on its registration and bound to the SSH identity the helper was first
//! allowed on. The helper is installed (or replaced by a newer build) under the
//! consented install root, started with `node serve` on an exec channel of a
//! dedicated SSH connection, and ends when that connection does: there is no
//! daemon, no listening socket and no background install.
//!
//! Admission is bounded per device: four requests run and thirty-two wait; a
//! request past that is refused as busy, never dropped. A request that was
//! sent and got no answer, because the connection ended or the deadline
//! passed, is `Unknown`: it may have taken effect, and the caller settles it
//! by reading the target again rather than by resending it (B14, B33).

use super::*;
#[path = "retirement.rs"]
mod retirement;
use hide_node_link::HostError;
pub use hide_node_link::LinkError;
use hide_node_link::device::{HostConsent, HostIdentity};
use hide_node_link::panes::{NodeEvent, PanesStarted};
use hide_node_link::protocol::{Call, Hello, PROTOCOL_VERSION, Request};
use hide_node_link::{LinkAnswer, NodeLink, call_as};
use russh_sftp::client::{RawSftpSession, error::Error as SftpError};
use russh_sftp::protocol::{FileAttributes, StatusCode};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Condvar;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc;

/// The longest answer line read from a device helper. The largest answer is
/// a 16 MiB document, which JSON escaping can grow by up to six times, so
/// this bounds memory without refusing any answer the protocol can produce.
const MAX_ANSWER_BYTES: usize = 128 * 1024 * 1024;

/// One answer line as the helper sends it (`hide_node_link::protocol::Response`),
/// with the result borrowed as JSON text rather than built into a `Value`.
#[derive(serde::Deserialize)]
struct AnswerLine<'a> {
    id: u64,
    // `Option` alone reads a `null` result as absent; a unit answer is `null`.
    #[serde(borrow, default, deserialize_with = "present")]
    ok: Option<&'a RawValue>,
    #[serde(default)]
    error: Option<HostError>,
}

/// What the reader hands a waiting call: the result's text or the refusal.
type Answered = Result<Box<RawValue>, HostError>;

fn present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<&'de RawValue>, D::Error> {
    serde::Deserialize::deserialize(deserializer).map(Some)
}

pub use hide_node_link::device::{
    DEFAULT_CLI_DIR, DEFAULT_HELPER_ROOT, EstablishError, Established, HOST_CONSENT_CARRIED_FROM,
    HOST_CONSENT_CONTRACT, Upload,
};

pub const MAX_RUNNING: usize = hide_host::serve::CONCURRENCY;
pub const MAX_QUEUED: usize = 32;

/// This program, which a device runs in its node role (`hided node serve`,
/// PRD core-host-node D-02).
const HELPER_NAME: &str = "hided";
/// The pane-side Workspace CLI, the same `hide` this daemon ships, so a
/// device's panes can reach this Hide through their return route.
const CLI_NAME: &str = "hide";
/// The rest of the install kit (`hide_kit`), under the names the kit reads
/// in its folder: the hook helper.
const HOOKS_NAME: &str = "hide-agent-hooks";
/// Bounds on a kit folder read into memory for upload (engineering rule
/// 15); a folder past them is left out and the kit names the missing part.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a draining connection waits for its admitted requests before it
/// closes anyway. An admitted request can wait up to its timeout to be
/// written and again for its answer, and the longest call timeout is 120 s
/// (an index walk, a worktree removal), so this outlasts both.
const DRAIN_BOUND: Duration = Duration::from_secs(250);
const HELLO_TIMEOUT: Duration = Duration::from_secs(15);

/// The helper builds this daemon carries, one per device platform.
#[derive(Clone, Debug, Default)]
pub struct HelperPackages {
    directory: Option<PathBuf>,
}

impl HelperPackages {
    pub fn new(directory: Option<PathBuf>) -> Self {
        Self { directory }
    }

    /// The build for `os`/`arch` (Rust's names: `macos`, `aarch64`). A
    /// `hided-<os>-<arch>` file wins; an unsuffixed `hided` is accepted only
    /// for this daemon's own platform, which is what the package and a
    /// development build carry: the daemon's own program.
    pub fn find(&self, os: &str, arch: &str) -> Result<PathBuf, String> {
        self.find_named(HELPER_NAME, os, arch).map_err(|()| {
            format!(
                "This Hide build does not include the device helper for {} {arch}; files and Git on this device stay unavailable until a build that packages it is installed",
                platform_label(os)
            )
        })
    }

    /// Everything this build puts on a device of `os`/`arch`: the helper,
    /// which the device cannot do without, and the kit's parts, each found by
    /// the same rule as the helper. A part this build does not carry is left out and named, and
    /// the kit on the device reports it as missing from the build.
    fn payload(&self, os: &str, arch: &str) -> Result<Payload, EstablishError> {
        let helper = self.find(os, arch).map_err(EstablishError::Unsupported)?;
        let helper = read_package(HELPER_NAME, &helper).map_err(EstablishError::Install)?;
        let mut payload = Payload {
            files: vec![helper],
            missing: Vec::new(),
        };
        for name in [CLI_NAME, HOOKS_NAME] {
            match self.find_named(name, os, arch) {
                Ok(path) => match read_package(name, &path) {
                    Ok(package) => payload.files.push(package),
                    Err(reason) => payload.missing.push(reason),
                },
                Err(()) => payload.missing.push(format!(
                    "This Hide build does not include {name} for {} {arch}",
                    platform_label(os)
                )),
            }
        }
        Ok(payload)
    }

    /// A file or a folder named `name`, by the helper's rule.
    fn find_named(&self, name: &str, os: &str, arch: &str) -> Result<PathBuf, ()> {
        let directory = self.directory.as_ref().ok_or(())?;
        let named = directory.join(format!("{name}-{os}-{arch}"));
        if named.exists() {
            return Ok(named);
        }
        let own = directory.join(name);
        if os == std::env::consts::OS && arch == std::env::consts::ARCH && own.exists() {
            return Ok(own);
        }
        Err(())
    }
}

/// One file of a build, as it goes into the device's version folder.
struct Package {
    /// Its path under the version folder, `/`-separated.
    relative: String,
    bytes: Vec<u8>,
    digest: String,
    executable: bool,
}

/// What a build puts on one device.
struct Payload {
    /// The helper first, then the kit's parts.
    files: Vec<Package>,
    /// Why a kit part is not among them.
    missing: Vec<String>,
}

impl Payload {
    /// The build's version: a digest over every file's path, mode and bytes,
    /// so any change to any part is a new folder on the device.
    fn version(&self) -> String {
        let mut entries = self
            .files
            .iter()
            .map(|file| {
                format!(
                    "{}\0{}\0{}\n",
                    file.relative,
                    if file.executable { "x" } else { "-" },
                    file.digest
                )
            })
            .collect::<Vec<_>>();
        entries.sort();
        hex_digest(entries.concat().as_bytes())
    }
}

/// A package keeps the mode bits it has here, which the device's copy gets.
/// A system without mode bits has nothing to give it, so the package is
/// refused there rather than installed with a guessed mode.
fn read_package(relative: &str, path: &Path) -> Result<Package, String> {
    use std::io::Read;
    let unreadable = |error: std::io::Error| {
        format!("The package {} could not be read: {error}", path.display())
    };
    let mut file = std::fs::File::open(path).map_err(unreadable)?;
    if !file.metadata().map_err(unreadable)?.is_file() {
        return Err(format!("The package {} is not a file", path.display()));
    }
    let mode = hide_platform::fs::permissions::Permissions::of(&file)
        .map_err(unreadable)?
        .unix_mode()
        .ok_or_else(|| {
            format!(
                "The package {} has no file mode on this system to install it with",
                path.display()
            )
        })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(unreadable)?;
    Ok(Package {
        relative: relative.to_owned(),
        digest: hex_digest(&bytes),
        bytes,
        executable: mode & 0o111 != 0,
    })
}

fn platform_label(os: &str) -> &str {
    match os {
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    }
}

/// `uname -s -m` in Rust's platform names.
fn platform_of(uname: &str) -> Result<(String, String), String> {
    let mut parts = uname.split_whitespace();
    let (Some(system), Some(machine)) = (parts.next(), parts.next()) else {
        return Err(format!(
            "The device reported an unreadable platform: {uname:?}"
        ));
    };
    let os = match system {
        "Darwin" => "macos",
        "Linux" => "linux",
        other => {
            return Err(format!(
                "The device runs {other}, which the helper does not support"
            ));
        }
    };
    let arch = match machine {
        "arm64" | "aarch64" => "aarch64",
        "x86_64" | "amd64" => "x86_64",
        other => {
            return Err(format!(
                "The device's processor {other} is not supported by the helper"
            ));
        }
    };
    Ok((os.to_owned(), arch.to_owned()))
}

/// What a device's node sends without being asked: its panes' credential
/// proofs and `hide` command streams (`hide_node_link::panes`). hided hears
/// them; the core never does. Both calls come from the link's reader, on the
/// transport's runtime: an implementation hands the work to its own thread
/// and returns, and never calls the link from inside them.
pub trait PaneEvents: Send + Sync {
    /// `event` arrived on `link`, the link of the device `node`.
    fn event(&self, node: &str, link: &RemoteHost, event: NodeEvent);
    /// `link` ended: nothing it vouched for holds any more (B18).
    fn closed(&self, node: &str, link: &RemoteHost);
}

/// Where a connector's links send their pane events, filled once hided's
/// server exists; events before that find it empty and are refused.
pub type PaneEventsSlot = Arc<std::sync::OnceLock<Arc<dyn PaneEvents>>>;

/// Which device a link serves and where its pane events go.
#[derive(Clone)]
pub struct PaneHook {
    pub node: String,
    pub events: PaneEventsSlot,
}

/// A live helper connection. Cloning shares it; the SSH connection ends when
/// the last clone is dropped or [`RemoteHost::close`] is called.
#[derive(Clone)]
pub struct RemoteHost {
    inner: Arc<Inner>,
}

impl fmt::Debug for RemoteHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteHost")
            .field("target", &self.inner.target)
            .finish_non_exhaustive()
    }
}

/// Admission to one helper connection: four running and thirty-two waiting
/// requests (PRD S5.5 D-15). Once the connection is draining or closed it
/// admits nothing new, and a request still waiting is refused rather than
/// sent, because nothing went out for it (B52).
struct Gate {
    state: Mutex<Admission>,
    changed: Condvar,
}

struct Admission {
    running: usize,
    queued: usize,
    stopped: Option<String>,
}

impl Gate {
    fn new() -> Self {
        Self {
            state: Mutex::new(Admission {
                running: 0,
                queued: 0,
                stopped: None,
            }),
            changed: Condvar::new(),
        }
    }

    fn admit(&self, timeout: Duration) -> Result<(), LinkError> {
        let mut admission = lock_recover(&self.state);
        if let Some(reason) = &admission.stopped {
            return Err(LinkError::NotConnected(reason.clone()));
        }
        if admission.running < MAX_RUNNING {
            admission.running += 1;
            return Ok(());
        }
        if admission.queued >= MAX_QUEUED {
            return Err(LinkError::Busy);
        }
        admission.queued += 1;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(reason) = &admission.stopped {
                let reason = reason.clone();
                admission.queued -= 1;
                drop(admission);
                self.changed.notify_all();
                return Err(LinkError::NotConnected(reason));
            }
            if admission.running < MAX_RUNNING {
                break;
            }
            let now = Instant::now();
            if now >= deadline {
                admission.queued -= 1;
                return Err(LinkError::Busy);
            }
            admission = self
                .changed
                .wait_timeout(admission, deadline - now)
                .map(|(guard, _)| guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner().0);
        }
        admission.queued -= 1;
        admission.running += 1;
        Ok(())
    }

    fn release(&self) {
        lock_recover(&self.state).running -= 1;
        self.changed.notify_all();
    }

    /// Why nothing more is admitted, once it is not.
    fn stopped(&self) -> Option<String> {
        lock_recover(&self.state).stopped.clone()
    }

    /// Admits nothing more; the first reason given is the one kept.
    fn stop(&self, reason: &str) {
        let mut admission = lock_recover(&self.state);
        if admission.stopped.is_none() {
            admission.stopped = Some(reason.to_owned());
        }
        drop(admission);
        self.changed.notify_all();
    }

    /// Waits until no admitted or waiting request remains, or `deadline`.
    fn wait_idle(&self, deadline: Instant) {
        let mut admission = lock_recover(&self.state);
        while admission.running > 0 || admission.queued > 0 {
            let now = Instant::now();
            if now >= deadline {
                return;
            }
            admission = self
                .changed
                .wait_timeout(admission, deadline - now)
                .map(|(guard, _)| guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner().0);
        }
    }
}

struct Inner {
    target: String,
    runtime: Arc<RemoteRuntime>,
    /// Wakes the reader to end the helper's channel: on [`RemoteHost::close`]
    /// and when the last clone is dropped. The device's connection stays,
    /// since other channels share it.
    closing: Arc<tokio::sync::Notify>,
    /// The link's place among the connection's session channels.
    _session_channel: tokio::sync::OwnedSemaphorePermit,
    writer: tokio::sync::Mutex<Pin<Box<dyn AsyncWrite + Send>>>,
    pending: Mutex<HashMap<u64, mpsc::Sender<Answered>>>,
    closed: Mutex<Option<String>>,
    gate: Gate,
    next_id: AtomicU64,
    /// Checkout roots this connection has opened, pinned to the directory
    /// they named then.
    roots: Mutex<HashMap<String, hide_node_link::RootIdentity>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.closing.notify_one();
    }
}

impl RemoteHost {
    pub fn target(&self) -> &str {
        &self.inner.target
    }

    /// Whether `other` is this same connection, not a later one to the
    /// same device.
    pub fn same_link(&self, other: &RemoteHost) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// A number that names this connection while it lives, for keying what
    /// belongs to it.
    pub fn identity(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }

    /// Why the connection ended, once it has.
    pub fn closed_reason(&self) -> Option<String> {
        lock_recover(&self.inner.closed).clone()
    }

    /// Ends the link; the helper exits when its channel closes. Requests
    /// still waiting for an answer become `Unknown`.
    pub fn close(&self, reason: &str) {
        mark_closed(&self.inner, reason.to_owned());
        self.inner.closing.notify_one();
    }

    /// Sends one request and waits at most `timeout` for its answer. Must
    /// not be called from inside an async context.
    pub fn call(&self, call: Call, timeout: Duration) -> Result<LinkAnswer, LinkError> {
        self.inner.gate.admit(timeout)?;
        let result = self.send_and_wait(call, timeout);
        self.inner.gate.release();
        result
    }

    fn send_and_wait(&self, call: Call, timeout: Duration) -> Result<LinkAnswer, LinkError> {
        let inner = &self.inner;
        let id = inner.next_id.fetch_add(1, Ordering::Relaxed);
        let mut line = serde_json::to_vec(&Request { id, call }).map_err(|error| {
            LinkError::NotConnected(format!("The request could not be encoded: {error}"))
        })?;
        line.push(b'\n');
        let (sender, receiver) = mpsc::channel();
        lock_recover(&inner.pending).insert(id, sender);
        if let Some(reason) = self.closed_reason() {
            lock_recover(&inner.pending).remove(&id);
            return Err(LinkError::NotConnected(reason));
        }
        // Waiting behind another request's write sends nothing, so running
        // out of time there leaves the connection as it was; only a write
        // that started and did not finish can have sent part of a line.
        let deadline = tokio::time::Instant::now() + timeout;
        let written = inner.runtime.block_on(async {
            let Ok(mut writer) = tokio::time::timeout_at(deadline, inner.writer.lock()).await
            else {
                return Err(LinkError::Busy);
            };
            // Admitted before the connection began to drain, but not sent:
            // it is refused now rather than sent after consent was withdrawn.
            if let Some(reason) = inner.gate.stopped() {
                return Err(LinkError::NotConnected(reason));
            }
            Ok(tokio::time::timeout_at(deadline, async {
                writer.write_all(&line).await?;
                writer.flush().await
            })
            .await)
        });
        let written = match written {
            Ok(written) => written,
            Err(unsent) => {
                lock_recover(&inner.pending).remove(&id);
                return Err(unsent);
            }
        };
        match written {
            Ok(Ok(())) => {}
            // A write that failed or timed out may have sent part of the
            // line, and the next request would be read as its tail, so the
            // connection ends here; the next use reconnects.
            Ok(Err(error)) => {
                lock_recover(&inner.pending).remove(&id);
                self.close("a request could not be written to the device helper");
                return Err(LinkError::Unknown(format!(
                    "The connection to the device failed while the request was sent ({error}); its result is unknown"
                )));
            }
            Err(_) => {
                lock_recover(&inner.pending).remove(&id);
                self.close("a request was not accepted by the device helper in time");
                return Err(LinkError::Unknown(
                    "The device did not accept the request in time; its result is unknown"
                        .to_owned(),
                ));
            }
        }
        match receiver.recv_timeout(timeout) {
            Ok(Ok(raw)) => Ok(LinkAnswer::Raw(raw)),
            Ok(Err(error)) => Err(LinkError::Refused(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                lock_recover(&inner.pending).remove(&id);
                Err(LinkError::Unknown(
                    "The device did not answer in time; the result is unknown and nothing was resent"
                        .to_owned(),
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(LinkError::Unknown(format!(
                "The connection to the device ended before it answered ({}); the result is unknown and nothing was resent",
                self.closed_reason()
                    .unwrap_or_else(|| "no reason reported".to_owned())
            ))),
        }
    }
}

impl NodeLink for RemoteHost {
    fn call(&self, call: Call, timeout: Duration) -> Result<LinkAnswer, LinkError> {
        RemoteHost::call(self, call, timeout)
    }

    fn close_when_idle(&self, reason: &str) {
        let host = RemoteHost {
            inner: Arc::clone(&self.inner),
        };
        // Nothing new goes out from here, whoever still holds the channel;
        // a request waiting for a slot is refused, since nothing was sent.
        self.inner.gate.stop(reason);
        let drained = reason.to_owned();
        let spawned = std::thread::Builder::new()
            .name("remote-host-drain".into())
            .spawn(move || {
                // Admitted requests are bounded by their own timeouts; this
                // bound only keeps a wedged count from holding the link open.
                host.inner.gate.wait_idle(Instant::now() + DRAIN_BOUND);
                host.close(&drained);
            });
        if let Err(error) = spawned {
            crate::diagnostic!(json!({
                "component": "remote_host",
                "kind": "host.drain_unstarted",
                "target": self.inner.target,
                "error": error.to_string(),
            }));
            self.close(reason);
        }
    }

    fn closed_reason(&self) -> Option<String> {
        RemoteHost::closed_reason(self)
    }

    fn close(&self, reason: &str) {
        RemoteHost::close(self, reason)
    }

    fn pinned(&self, root: &str) -> Option<hide_node_link::RootIdentity> {
        lock_recover(&self.inner.roots).get(root).copied()
    }

    fn pin(&self, root: &str, identity: Option<hide_node_link::RootIdentity>) {
        let mut roots = lock_recover(&self.inner.roots);
        match identity {
            Some(identity) => {
                roots.insert(root.to_owned(), identity);
            }
            None => {
                roots.remove(root);
            }
        }
    }
}

fn mark_closed(inner: &Inner, reason: String) {
    let mut closed = lock_recover(&inner.closed);
    if closed.is_none() {
        *closed = Some(reason.clone());
    }
    drop(closed);
    // Dropping the senders wakes every waiting request as disconnected.
    lock_recover(&inner.pending).clear();
    inner.gate.stop(&reason);
}

/// Connects, checks consent against the device that answered, installs the
/// helper when the device lacks this build's, and starts it. Blocking; run
/// it off the runtime lock.
pub fn establish(
    client: &RusshRemoteClient,
    packages: &HelperPackages,
    consent: &HostConsent,
    retirement_projects: &[String],
    panes: Option<PaneHook>,
    on_close: Box<dyn FnOnce(String) + Send + 'static>,
) -> Result<Established, EstablishError> {
    let session = client
        .runtime
        .block_on(async {
            tokio::time::timeout(SSH_OPERATION_TIMEOUT, client.shared_session())
                .await
                .unwrap_or_else(|_| {
                    Err(remote_error(
                        "remote-host-connect",
                        &client.host.host_id,
                        RemoteStage::Ssh,
                        "the SSH connection timed out",
                        true,
                        false,
                    ))
                })
        })
        .map_err(EstablishError::Connect)?;
    let identity = HostIdentity {
        user: client.host.user.clone(),
        hostname: client.host.hostname.clone(),
        port: client.host.port,
        host_key_sha256: client.observed_host_key().unwrap_or_default(),
    };
    if identity.host_key_sha256.is_empty() {
        return Err(EstablishError::Helper(
            "The device's host key was not observed, so consent cannot be checked".to_owned(),
        ));
    }
    if let Some(bound) = consent.identity.as_ref()
        && bound != &identity
    {
        return Err(EstablishError::IdentityChanged {
            bound: Box::new(bound.clone()),
            observed: Box::new(identity),
        });
    }
    let result = client.runtime.block_on(async {
        tokio::time::timeout(
            INSTALL_TIMEOUT,
            start_helper(
                &session,
                client,
                packages,
                &consent.helper_root,
                retirement_projects,
            ),
        )
        .await
        .unwrap_or_else(|_| {
            Err(EstablishError::Install(
                "Installing the device helper timed out; nothing was started".to_owned(),
            ))
        })
    });
    // A failed setup leaves the device's connection to the channels that
    // share it; what it opened closed with it.
    let Started {
        channel,
        session_channel,
        installed,
        helper_path,
        upload,
    } = result?;
    let host = spawn_host(client, channel, session_channel, panes.clone(), on_close);
    let hello: Hello = call_as(&host, Call::Hello, HELLO_TIMEOUT).map_err(|error| {
        EstablishError::Helper(format!("The device helper did not start: {error}"))
    })?;
    if let Some(reason) = helper_protocol_refusal(&hello) {
        host.close("helper protocol mismatch");
        return Err(EstablishError::Helper(reason));
    }
    if panes.is_some() {
        start_panes(client, &host);
    }
    Ok(Established {
        host: Arc::new(host),
        identity,
        hello,
        installed,
        helper_path,
        upload,
    })
}

/// Starts the node's pane service for the device's Herdr, so its panes'
/// `hide` reaches the core over this link. A device whose Herdr cannot be
/// found keeps its files and Git; its panes' `hide` answers that the node is
/// unavailable, and the reason goes to the log.
fn start_panes(client: &RusshRemoteClient, host: &RemoteHost) {
    let started = client
        .herdr_socket_path()
        .map_err(|error| error.to_string())
        .and_then(|herdr_socket| {
            call_as::<PanesStarted>(host, Call::PanesStart { herdr_socket }, HELLO_TIMEOUT)
                .map_err(|error| error.to_string())
        });
    match started {
        Ok(_) => crate::diagnostic!(json!({
            "component": "remote_host",
            "kind": "host.panes_started",
            "target": host.inner.target,
        })),
        Err(reason) => crate::diagnostic!(json!({
            "component": "remote_host",
            "kind": "host.panes_unstarted",
            "target": host.inner.target,
            "reason": reason,
        })),
    }
}

/// Why a started helper is not used: one that answers another protocol reads
/// this build's requests with other shapes (a helper on protocol 8 ignores
/// the View diffs a `changes` read carries and answers none, PRD S7 A5).
/// The refusal is the device's unavailable reason. A device always runs the
/// helper this Hide carries, installed by the digest of its bytes, so only a
/// rebuilt or reinstalled Hide clears it: a development `hided` carries the
/// `hided` it runs as (`host_helper_dir`), and a
/// stale build there is installed and refused again on every connection.
fn helper_protocol_refusal(hello: &Hello) -> Option<String> {
    (hello.protocol != PROTOCOL_VERSION).then(|| {
        format!(
            "The device helper speaks protocol {}, this Hide needs {PROTOCOL_VERSION}; the helper this Hide carries does not match it, so rebuild or reinstall Hide",
            hello.protocol
        )
    })
}

/// A started helper: its channel and that channel's place among the
/// connection's sessions, and what the install did.
struct Started {
    channel: Channel<Msg>,
    session_channel: tokio::sync::OwnedSemaphorePermit,
    installed: bool,
    helper_path: String,
    upload: Upload,
}

async fn start_helper(
    session: &Handle<KnownHostHandler>,
    client: &RusshRemoteClient,
    packages: &HelperPackages,
    helper_root: &str,
    retirement_projects: &[String],
) -> Result<Started, EstablishError> {
    let target = client.host.host_id.clone();
    let admit = |operation: &'static str, stage: RemoteStage| async move {
        client
            .session_channel(operation, stage)
            .await
            .map_err(EstablishError::Connect)
    };
    let permit = admit("remote-host-platform", RemoteStage::Sftp).await?;
    let uname = execute_channel(
        session,
        "uname -s -m",
        "remote-host-platform",
        &target,
        RemoteStage::Sftp,
    )
    .await
    .map_err(EstablishError::Connect)?;
    if uname.exit_status != 0 {
        return Err(EstablishError::Unsupported(format!(
            "The device could not report its platform: {}",
            uname.stderr.trim()
        )));
    }
    drop(permit);
    let (os, arch) = platform_of(uname.stdout.trim()).map_err(EstablishError::Unsupported)?;
    let payload = packages.payload(&os, &arch)?;
    // The helper inherits this SSH exec environment. Read its path overrides
    // before uploading any candidate; older helpers need no new operation.
    let permit = admit("remote-retirement-environment", RemoteStage::Sftp).await?;
    let environment = execute_channel(
        session,
        r#"[ "${#HCOORD_HOME}" -le 4096 ] && [ "${#HIDE_STATE_DIR}" -le 4096 ] && [ "${#XDG_STATE_HOME}" -le 4096 ] || exit 65; printf '%s\n' "${HCOORD_HOME-}" "${HIDE_STATE_DIR-}" "${XDG_STATE_HOME-}""#,
        "remote-retirement-environment",
        &target,
        RemoteStage::Sftp,
    )
    .await
    .map_err(EstablishError::Connect)?;
    if environment.exit_status != 0 {
        return Err(EstablishError::Install("The device retirement paths could not be inspected; check its SSH environment and retry; no helper was uploaded".into()));
    }
    drop(permit);
    let locations = retirement::Locations::from_environment(&environment.stdout)?;

    let permit = admit("remote-host-install", RemoteStage::Sftp).await?;
    let channel = session.channel_open_session().await.map_err(|error| {
        EstablishError::Install(format!("The SFTP channel could not be opened: {error}"))
    })?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|error| {
            EstablishError::Install(format!("SFTP is not available on the device: {error}"))
        })?;
    let raw = RawSftpSession::new(channel.into_stream());
    raw.set_timeout(30);
    let installed = install(&raw, helper_root, &payload, retirement_projects, &locations).await;
    let _ = raw.close_session();
    drop(raw);
    drop(permit);
    let installed = installed?;
    let (helper_path, fresh) = (installed.helper_path, installed.fresh);

    let session_channel = admit("remote-host-start", RemoteStage::Ssh).await?;
    let channel = session.channel_open_session().await.map_err(|error| {
        EstablishError::Helper(format!("The helper channel could not be opened: {error}"))
    })?;
    channel
        .exec(true, format!("{} node serve", shell_quote(&helper_path)))
        .await
        .map_err(|error| {
            EstablishError::Helper(format!("The helper could not be started: {error}"))
        })?;
    Ok(Started {
        channel,
        session_channel,
        installed: fresh,
        helper_path,
        upload: installed.upload,
    })
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn sftp_failure(what: &str, error: impl fmt::Display) -> EstablishError {
    EstablishError::Install(format!("{what}: {error}"))
}

/// Where an install left things.
struct Installed {
    helper_path: String,
    /// Whether the helper itself was written on this connection.
    fresh: bool,
    upload: Upload,
}

/// Puts this build's helper and kit parts in `<root>/<version>/`, where the
/// version is a prefix of the digest over all of them, keeping the folder
/// layout the kit reads. The
/// helper is reused only after checking its bytes, because Hide runs it; any
/// other file is reused when a file of its size that only the account can
/// change is already there, because only a verified upload is ever renamed
/// into that private folder and reading every part back on each connection
/// would delay every reconnect. So an install that stopped part way resumes
/// with the files it had not sent (B18). A kit file that cannot be placed
/// never stops the helper: the kit on the device reports that part. The
/// helper removes older builds once it has pointed `current` at this one
/// (`hide_host::kit`).
async fn install(
    raw: &RawSftpSession,
    helper_root: &str,
    payload: &Payload,
    retirement_projects: &[String],
    locations: &retirement::Locations,
) -> Result<Installed, EstablishError> {
    raw.init()
        .await
        .map_err(|error| sftp_failure("SFTP did not start", error))?;
    let home = raw
        .realpath(".")
        .await
        .map_err(|error| sftp_failure("SFTP did not report the home directory", error))?
        .files
        .first()
        .map(|entry| entry.filename.clone())
        .ok_or_else(|| {
            EstablishError::Install("SFTP did not report the home directory".to_owned())
        })?;
    if !home.starts_with('/') || home.chars().any(char::is_control) {
        return Err(EstablishError::Install(
            "SFTP reported an unusable home directory".to_owned(),
        ));
    }
    let owner = raw
        .lstat(&home)
        .await
        .map_err(|error| sftp_failure("The home directory could not be inspected", error))?
        .attrs
        .uid
        .ok_or_else(|| {
            EstablishError::Install("SFTP did not report the home directory's owner".to_owned())
        })?;
    let root = match helper_root.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", home.trim_end_matches('/')),
        None if helper_root.starts_with('/') => helper_root.to_owned(),
        None => {
            return Err(EstablishError::Install(format!(
                "The helper install root {helper_root:?} must be absolute or start with ~/"
            )));
        }
    };
    if root.split('/').any(|part| part == ".." || part == ".") || root.chars().any(char::is_control)
    {
        return Err(EstablishError::Install(
            "The helper install root is not a plain path".to_owned(),
        ));
    }
    // No folders, staging files or current link have changed at this point.
    tokio::time::timeout(
        SSH_OPERATION_TIMEOUT,
        retirement::preflight(raw, &home, owner, retirement_projects, locations),
    ).await.map_err(|_| EstablishError::Install(
        "The device retirement preflight timed out; inspect its run and request state and retry; no helper was uploaded".into()
    ))??;
    // Everything from here goes through the path the check resolved, whose
    // every folder was checked and none is a link, so a link on the spelled
    // path cannot be swapped between the check and the launch.
    let root = ensure_private_dirs(raw, &home, &root, owner).await?;
    let version = payload.version();
    let version_dir = format!("{root}/{}", &version[..16]);
    ensure_private_dir(raw, &version_dir, owner).await?;
    let mut upload = Upload {
        missing: payload.missing.clone(),
        ..Upload::default()
    };
    let (helper, parts) = payload
        .files
        .split_first()
        .ok_or_else(|| EstablishError::Install("The build carries no helper".to_owned()))?;
    let final_path = format!("{version_dir}/{}", helper.relative);
    let reusable = match raw.lstat(&final_path).await {
        Ok(existing) => {
            private_file(&existing.attrs, owner)
                && existing.attrs.size == Some(helper.bytes.len() as u64)
                && remote_digest(raw, &final_path, helper.bytes.len(), "helper")
                    .await
                    .ok()
                    .as_deref()
                    == Some(helper.digest.as_str())
        }
        Err(_) => false,
    };
    let (helper_path, fresh) = if reusable {
        upload.reused += 1;
        (final_path, false)
    } else {
        upload.sent += 1;
        (
            place_file(raw, &version_dir, helper, owner, "helper").await?,
            true,
        )
    };
    let mut folders = std::collections::HashSet::new();
    for part in parts {
        let placed = async {
            // Each folder on the way is made private before anything is put
            // in it, parents first.
            let mut folder = version_dir.clone();
            if let Some((parents, _)) = part.relative.rsplit_once('/') {
                for name in parents.split('/') {
                    folder = format!("{folder}/{name}");
                    if folders.insert(folder.clone()) {
                        ensure_private_dir(raw, &folder, owner).await?;
                    }
                }
            }
            let path = format!("{version_dir}/{}", part.relative);
            match raw.lstat(&path).await {
                Ok(existing)
                    if private_file(&existing.attrs, owner)
                        && existing.attrs.size == Some(part.bytes.len() as u64) =>
                {
                    Ok(false)
                }
                _ => place_file(raw, &version_dir, part, owner, &part.relative)
                    .await
                    .map(|_| true),
            }
        }
        .await;
        match placed {
            Ok(true) => upload.sent += 1,
            Ok(false) => upload.reused += 1,
            Err(error) => upload.missing.push(error.to_string()),
        }
    }
    Ok(Installed {
        helper_path,
        fresh,
        upload,
    })
}

/// Numbers each upload this process stages, so two connections to one
/// account (two registrations of the same machine) never share a staging
/// name.
static NEXT_UPLOAD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Whether `path` already holds exactly `package`'s bytes.
async fn holds(raw: &RawSftpSession, path: &str, package: &Package, what: &str) -> bool {
    matches!(
        remote_digest(raw, path, package.bytes.len(), what).await,
        Ok(found) if found == package.digest
    )
}

/// Uploads `package` beside its final name, checks the uploaded bytes, and
/// renames it into place, so the final name only ever holds a whole,
/// verified copy. Another connection to the same account can place the same
/// build at the same time: a final name that already holds these bytes is
/// kept rather than replaced.
async fn place_file(
    raw: &RawSftpSession,
    version_dir: &str,
    package: &Package,
    owner: u32,
    what: &str,
) -> Result<String, EstablishError> {
    let final_path = format!("{version_dir}/{}", package.relative);
    let (folder, name) = final_path
        .rsplit_once('/')
        .unwrap_or((version_dir, package.relative.as_str()));
    let staging = format!(
        "{folder}/.upload-{name}-{}-{}",
        std::process::id(),
        NEXT_UPLOAD.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    // Only a process that ended with this same pid could have left it.
    let _ = raw.remove(&staging).await;
    let handle = raw
        .open(
            &staging,
            OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
            FileAttributes {
                permissions: Some(if package.executable { 0o700 } else { 0o600 }),
                ..FileAttributes::empty()
            },
        )
        .await
        .map_err(|error| sftp_failure(&format!("The {what} could not be uploaded"), error))?
        .handle;
    let written = async {
        for (index, chunk) in package.bytes.chunks(32 * 1024).enumerate() {
            raw.write(&handle, (index * 32 * 1024) as u64, chunk.to_vec())
                .await
                .map_err(|error| sftp_failure(&format!("The {what} upload failed"), error))?;
        }
        Ok::<(), EstablishError>(())
    }
    .await;
    let closed = raw.close(handle).await;
    let verified = match (written, closed) {
        (Ok(()), Ok(_)) => match remote_digest(raw, &staging, package.bytes.len(), what).await {
            Ok(uploaded) if uploaded == package.digest => Ok(()),
            Ok(_) => Err(EstablishError::Install(format!(
                "The uploaded {what} did not match the package; it was not used"
            ))),
            Err(error) => Err(error),
        },
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(sftp_failure(
            &format!("The {what} upload did not finish"),
            error,
        )),
    };
    if let Err(error) = verified {
        let _ = raw.remove(&staging).await;
        return Err(error);
    }
    if holds(raw, &final_path, package, what).await {
        let _ = raw.remove(&staging).await;
    } else {
        let _ = raw.remove(&final_path).await;
        if let Err(error) = raw.rename(&staging, &final_path).await {
            let _ = raw.remove(&staging).await;
            if !holds(raw, &final_path, package, what).await {
                return Err(sftp_failure(
                    &format!("The {what} could not be put in place"),
                    error,
                ));
            }
        }
    }
    let placed = raw.lstat(&final_path).await.map_err(|error| {
        sftp_failure(
            &format!("The installed {what} could not be inspected"),
            error,
        )
    })?;
    if !private_file(&placed.attrs, owner) {
        let _ = raw.remove(&final_path).await;
        return Err(EstablishError::Install(format!(
            "{final_path} was not left a file only the account can change, so the {what} was not used"
        )));
    }
    Ok(final_path)
}

/// A helper file the account owns and no group or other account can write.
/// Its folder is already private, so this refuses what a device's own
/// defaults could leave behind, not what another account could swap in.
fn private_file(attrs: &FileAttributes, owner: u32) -> bool {
    attrs.is_regular()
        && attrs.uid == Some(owner)
        && attrs.permissions.is_some_and(|mode| mode & 0o022 == 0)
}

/// Creates and checks the helper root, and answers the real path it resolves
/// to, which is the one the helper is installed under and started from.
async fn ensure_private_dirs(
    raw: &RawSftpSession,
    home: &str,
    root: &str,
    owner: u32,
) -> Result<String, EstablishError> {
    // Components below home are created private; home itself is not touched.
    // The prefix is compared by path component, so `/home/al` is not a
    // prefix of `/home/alice`.
    let (mut current, relative) = match Path::new(root).strip_prefix(home) {
        Ok(below) => (
            home.trim_end_matches('/').to_owned(),
            below.to_string_lossy().into_owned(),
        ),
        Err(_) => (String::new(), root.to_owned()),
    };
    for part in relative.split('/').filter(|part| !part.is_empty()) {
        current.push('/');
        current.push_str(part);
        // An existing ancestor may be a link (`/tmp` on macOS), but the link
        // and the folder it leads to must both be ones no other account can
        // change, or that account could swap the helper between its digest
        // check and its launch (OpenSSH's rule for key files). The root
        // itself is checked without following links below.
        match raw.lstat(&current).await {
            Ok(link) if link.attrs.is_symlink() && !owned_by(&link.attrs, owner) => {
                return Err(EstablishError::Install(format!(
                    "{current} is a link another account owns, so the helper was not installed"
                )));
            }
            Ok(_) | Err(SftpError::Status(_)) => {}
            Err(error) => {
                return Err(sftp_failure(
                    "The helper folder could not be inspected",
                    error,
                ));
            }
        }
        match raw.stat(&current).await {
            Ok(attrs) if attrs.attrs.is_dir() => validate_ancestor(&attrs.attrs, owner, &current)?,
            Ok(_) => {
                return Err(EstablishError::Install(format!(
                    "{current} exists and is not a folder, so the helper was not installed"
                )));
            }
            Err(SftpError::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
                make_private_dir(raw, &current).await?;
            }
            Err(error) => {
                return Err(sftp_failure(
                    "The helper folder could not be inspected",
                    error,
                ));
            }
        }
    }
    let attrs = raw
        .lstat(root)
        .await
        .map_err(|error| sftp_failure("The helper folder could not be inspected", error))?
        .attrs;
    validate_private(&attrs, owner, root)?;
    // The walk above checked each folder as spelled, and home not at all.
    // The folder the root really is is checked again from `/`, home and its
    // parents included, so a linked ancestor whose target sits under a
    // folder another account can change is refused as well (OpenSSH's
    // `safe_path` rule for key files, which a device that signs in with a
    // key already passes).
    let resolved = raw
        .realpath(root)
        .await
        .map_err(|error| sftp_failure("The helper folder could not be resolved", error))?
        .files
        .first()
        .map(|entry| entry.filename.clone())
        .ok_or_else(|| {
            EstablishError::Install("SFTP did not resolve the helper folder".to_owned())
        })?;
    for ancestor in ancestors(&resolved)? {
        let attrs = raw
            .lstat(&ancestor)
            .await
            .map_err(|error| sftp_failure("The helper folder could not be inspected", error))?
            .attrs;
        if !attrs.is_dir() {
            return Err(EstablishError::Install(format!(
                "{ancestor} is not a folder on the resolved helper path, so the helper was not installed"
            )));
        }
        validate_ancestor(&attrs, owner, &ancestor)?;
    }
    let attrs = raw
        .lstat(&resolved)
        .await
        .map_err(|error| sftp_failure("The helper folder could not be inspected", error))?
        .attrs;
    validate_private(&attrs, owner, &resolved)?;
    Ok(resolved)
}

/// Every folder above `resolved`, from `/`, for an absolute path SFTP
/// resolved; anything else is refused rather than guessed at.
fn ancestors(resolved: &str) -> Result<Vec<String>, EstablishError> {
    if !resolved.starts_with('/')
        || resolved.chars().any(char::is_control)
        || resolved.split('/').any(|part| part == ".." || part == ".")
    {
        return Err(EstablishError::Install(format!(
            "SFTP resolved the helper folder to an unusable path {resolved:?}"
        )));
    }
    let parts: Vec<&str> = resolved
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let mut folders = vec!["/".to_owned()];
    let mut current = String::new();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        current.push('/');
        current.push_str(part);
        folders.push(current.clone());
    }
    Ok(folders)
}

/// Makes `path` a 0700 folder. Another connection to the same account (two
/// registrations of one machine connecting together) can make it first: a
/// folder that is there when `mkdir` fails is taken as that one, and the
/// caller checks it as it would its own.
async fn make_private_dir(raw: &RawSftpSession, path: &str) -> Result<(), EstablishError> {
    let made = raw
        .mkdir(
            path,
            FileAttributes {
                permissions: Some(0o700),
                ..FileAttributes::empty()
            },
        )
        .await;
    match made {
        Ok(_) => Ok(()),
        Err(error) => match raw.lstat(path).await {
            Ok(found) if found.attrs.is_dir() => Ok(()),
            _ => Err(sftp_failure(
                "The helper folder could not be created",
                error,
            )),
        },
    }
}

async fn ensure_private_dir(
    raw: &RawSftpSession,
    path: &str,
    owner: u32,
) -> Result<(), EstablishError> {
    match raw.lstat(path).await {
        Ok(attrs) => validate_private(&attrs.attrs, owner, path),
        Err(SftpError::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
            make_private_dir(raw, path).await?;
            let attrs = raw
                .lstat(path)
                .await
                .map_err(|error| sftp_failure("The helper folder could not be inspected", error))?;
            validate_private(&attrs.attrs, owner, path)
        }
        Err(error) => Err(sftp_failure(
            "The helper folder could not be inspected",
            error,
        )),
    }
}

fn owned_by(attrs: &FileAttributes, owner: u32) -> bool {
    attrs.uid == Some(owner) || attrs.uid == Some(0)
}

/// A folder on the way to the helper root: owned by the account or by root,
/// and writable by no group or other account unless it is a root-owned sticky
/// folder such as `/tmp`, where others cannot rename what the account made.
/// A folder whose mode was not reported is refused, not assumed safe. Group
/// write is refused even for a group only the account is in (a `umask 002`
/// home on some Linux systems), as OpenSSH's default rule does; the refusal
/// names the fix.
fn validate_ancestor(attrs: &FileAttributes, owner: u32, path: &str) -> Result<(), EstablishError> {
    let Some(mode) = attrs.permissions else {
        return Err(EstablishError::Install(format!(
            "The device did not report who can change {path}, so the helper was not installed under it"
        )));
    };
    let shared = mode & 0o022 != 0;
    let root_sticky = attrs.uid == Some(0) && mode & 0o1000 != 0;
    if owned_by(attrs, owner) && (!shared || root_sticky) {
        return Ok(());
    }
    Err(EstablishError::Install(if owned_by(attrs, owner) {
        format!(
            "{path} can be changed by its group or by other accounts, so the helper was not installed under it; remove that write access on the device (chmod go-w {path}) or choose another install root"
        )
    } else {
        format!(
            "{path} belongs to another account, so the helper was not installed under it; choose another install root"
        )
    }))
}

fn validate_private(attrs: &FileAttributes, owner: u32, path: &str) -> Result<(), EstablishError> {
    if !attrs.is_dir()
        || attrs.uid != Some(owner)
        || attrs.permissions.is_none_or(|mode| mode & 0o022 != 0)
    {
        return Err(EstablishError::Install(format!(
            "{path} must be a folder owned by the account and not writable by others; the helper was not installed"
        )));
    }
    Ok(())
}

async fn remote_digest(
    raw: &RawSftpSession,
    path: &str,
    length: usize,
    what: &str,
) -> Result<String, EstablishError> {
    let handle = raw
        .open(path, OpenFlags::READ, FileAttributes::empty())
        .await
        .map_err(|error| sftp_failure(&format!("The {what} could not be read back"), error))?
        .handle;
    let mut hasher = Sha256::new();
    let mut offset = 0usize;
    let result = async {
        while offset < length {
            let data = raw
                .read(
                    &handle,
                    offset as u64,
                    (length - offset).min(32 * 1024) as u32,
                )
                .await
                .map_err(|error| {
                    sftp_failure(&format!("The {what} could not be read back"), error)
                })?
                .data;
            if data.is_empty() {
                break;
            }
            offset += data.len();
            hasher.update(&data);
        }
        Ok::<(), EstablishError>(())
    }
    .await;
    let _ = raw.close(handle).await;
    result?;
    if offset != length {
        return Err(EstablishError::Install(format!(
            "The {what} on the device is incomplete"
        )));
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn spawn_host(
    client: &RusshRemoteClient,
    channel: Channel<Msg>,
    session_channel: tokio::sync::OwnedSemaphorePermit,
    panes: Option<PaneHook>,
    on_close: Box<dyn FnOnce(String) + Send + 'static>,
) -> RemoteHost {
    let writer: Pin<Box<dyn AsyncWrite + Send>> = Box::pin(channel.make_writer());
    let closing = Arc::new(tokio::sync::Notify::new());
    let inner = Arc::new(Inner {
        target: client.host.host_id.clone(),
        runtime: Arc::clone(&client.runtime),
        closing: Arc::clone(&closing),
        _session_channel: session_channel,
        writer: tokio::sync::Mutex::new(writer),
        pending: Mutex::new(HashMap::new()),
        closed: Mutex::new(None),
        gate: Gate::new(),
        next_id: AtomicU64::new(1),
        roots: Mutex::new(HashMap::new()),
    });
    let reader = Arc::downgrade(&inner);
    client.runtime.spawn(async move {
        let mut channel = channel;
        let mut buffer: Vec<u8> = Vec::new();
        // How far `buffer` has been searched for a line end, so a large
        // answer arriving in many chunks is scanned once.
        let mut scanned = 0;
        let mut stderr: Vec<u8> = Vec::new();
        let reason = loop {
            let message = tokio::select! {
                message = channel.wait() => message,
                () = closing.notified() => break "this Hide closed the link".to_owned(),
            };
            match message {
                Some(ChannelMsg::Data { data }) => {
                    buffer.extend_from_slice(&data);
                    while let Some(offset) = buffer[scanned..].iter().position(|byte| *byte == b'\n') {
                        let end = scanned + offset;
                        scanned = 0;
                        let line: Vec<u8> = buffer.drain(..=end).collect();
                        let Some(inner) = reader.upgrade() else { return };
                        if line.starts_with(b"{\"event\":") {
                            deliver_event(&inner, panes.as_ref(), &line);
                            continue;
                        }
                        // The answer is only tokenized here and kept as its
                        // own text; one nobody waits for is never copied.
                        match serde_json::from_slice::<AnswerLine<'_>>(&line) {
                            Ok(answer) => {
                                let outcome = match (answer.ok, answer.error) {
                                    (Some(raw), None) => Some(Ok(raw)),
                                    (None, Some(error)) => Some(Err(error)),
                                    _ => None,
                                };
                                let sender = lock_recover(&inner.pending).remove(&answer.id);
                                match (sender, outcome) {
                                    (Some(sender), Some(outcome)) => {
                                        let _ = sender.send(outcome.map(ToOwned::to_owned));
                                    }
                                    (sender, _) => crate::diagnostic!(json!({
                                        "component": "remote_host",
                                        "kind": "host.answer_unmatched",
                                        "target": inner.target,
                                        "id": answer.id,
                                        "awaited": sender.is_some(),
                                        "bytes": line.len(),
                                    })),
                                }
                            }
                            // serde's own text can quote the value it could
                            // not read, which may be file contents, so only
                            // its class and position are logged (S5.5 B48).
                            Err(error) => crate::diagnostic!(json!({
                                "component": "remote_host",
                                "kind": "host.answer_unreadable",
                                "target": inner.target,
                                "class": format!("{:?}", error.classify()),
                                "line": error.line(),
                                "column": error.column(),
                                "bytes": line.len(),
                            })),
                        }
                    }
                    scanned = buffer.len();
                    // The helper's answers are untrusted input: a line that
                    // outgrows every answer the protocol allows ends the
                    // connection rather than this process's memory.
                    if buffer.len() > MAX_ANSWER_BYTES {
                        break format!(
                            "the device helper sent an answer longer than {} MiB",
                            MAX_ANSWER_BYTES / (1024 * 1024)
                        );
                    }
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    stderr.extend_from_slice(&data);
                    if stderr.len() > 4096 {
                        stderr.drain(..stderr.len() - 4096);
                    }
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    break format!("the device helper exited with status {exit_status}");
                }
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                    break "the connection to the device helper closed".to_owned();
                }
                Some(_) => {}
            }
        };
        // Whatever ended the loop, the helper's channel ends with it, so the
        // helper reads the end of its input and exits.
        let _ = channel.eof().await;
        let _ = channel.close().await;
        if let Some(inner) = reader.upgrade() {
            crate::diagnostic!(json!({
                "component": "remote_host",
                "kind": "host.closed",
                "target": inner.target,
                "reason": reason,
                "stderr_tail": String::from_utf8_lossy(&stderr).chars().rev().take(400).collect::<String>().chars().rev().collect::<String>(),
            }));
            mark_closed(&inner, reason.clone());
            if let Some(hook) = &panes
                && let Some(events) = hook.events.get()
            {
                events.closed(&hook.node, &RemoteHost { inner });
            }
        }
        on_close(reason);
    });
    RemoteHost { inner }
}

/// Hands one event line to hided's pane events. A node that sends events
/// on a link nobody listens to, or a line that is not one, is logged and
/// dropped; the requests on the link go on.
fn deliver_event(inner: &Arc<Inner>, panes: Option<&PaneHook>, line: &[u8]) {
    let event = match serde_json::from_slice::<NodeEvent>(line) {
        Ok(event) => event,
        Err(error) => {
            crate::diagnostic!(json!({
                "component": "remote_host",
                "kind": "host.event_unreadable",
                "target": inner.target,
                "class": format!("{:?}", error.classify()),
                "bytes": line.len(),
            }));
            return;
        }
    };
    match panes.and_then(|hook| Some((hook, hook.events.get()?))) {
        Some((hook, events)) => events.event(
            &hook.node,
            &RemoteHost {
                inner: Arc::clone(inner),
            },
            event,
        ),
        None => crate::diagnostic!(json!({
            "component": "remote_host",
            "kind": "host.event_unheard",
            "target": inner.target,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(uid: u32, mode: u32) -> FileAttributes {
        FileAttributes {
            uid: Some(uid),
            permissions: Some(0o040000 | mode),
            ..FileAttributes::empty()
        }
    }

    /// Waits until a request waits for a slot. Queueing wakes no one, so
    /// the count is the only state to watch.
    #[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
    fn queued(gate: &Gate, what: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while lock_recover(&gate.state).queued == 0 {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// B52: withdrawing consent stops the channel admitting anything new,
    /// whoever still holds it, and refuses a request waiting for a slot
    /// unsent, while the requests already admitted run to their answers and
    /// the connection waits for them before it closes.
    #[test]
    #[allow(clippy::disallowed_methods)] // a window in which the idle wait must not end: no state reports an event that has not happened
    fn a_draining_connection_refuses_new_and_waiting_requests_and_waits_for_running_ones() {
        let gate = Arc::new(Gate::new());
        for _ in 0..MAX_RUNNING {
            assert!(gate.admit(Duration::from_secs(1)).is_ok());
        }
        let waiting = {
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || gate.admit(Duration::from_secs(60)))
        };
        queued(&gate, "the request never waited");

        let stopped = Instant::now();
        gate.stop("consent revoked");
        assert!(matches!(
            waiting.join().unwrap(),
            Err(LinkError::NotConnected(reason)) if reason == "consent revoked"
        ));
        assert!(stopped.elapsed() < Duration::from_secs(10));
        assert!(matches!(
            gate.admit(Duration::from_secs(1)),
            Err(LinkError::NotConnected(_))
        ));

        let (started, waiting_idle) = std::sync::mpsc::channel();
        let idle = {
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                started.send(()).unwrap();
                gate.wait_idle(Instant::now() + Duration::from_secs(60))
            })
        };
        // The window covers only the wait itself, not the thread's start.
        waiting_idle.recv_timeout(Duration::from_secs(10)).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!idle.is_finished(), "closed while admitted requests ran");
        for _ in 0..MAX_RUNNING {
            gate.release();
        }
        idle.join().unwrap();
        let admission = lock_recover(&gate.state);
        assert_eq!((admission.running, admission.queued), (0, 0));
    }

    /// PRD S7 A5: a helper still on the protocol before the View diffs is
    /// refused with a reason naming both versions, never used.
    #[test]
    fn a_helper_on_the_previous_protocol_is_refused_with_both_versions() {
        let hello = |protocol| Hello {
            protocol,
            version: "0.0.0".to_owned(),
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
            home: None,
            machine_identity: hide_node_link::protocol::MachineIdentity::Unavailable {
                reason: "fixture identity is unavailable".to_owned(),
            },
        };
        let refusal = helper_protocol_refusal(&hello(PROTOCOL_VERSION - 1)).expect("refused");
        assert_eq!(
            refusal,
            format!(
                "The device helper speaks protocol {}, this Hide needs {PROTOCOL_VERSION}; the helper this Hide carries does not match it, so rebuild or reinstall Hide",
                PROTOCOL_VERSION - 1
            )
        );
        assert_eq!(helper_protocol_refusal(&hello(PROTOCOL_VERSION)), None);
    }

    /// An answer line keeps its result as the helper's own text, `null`
    /// included, and a refusal as the helper's error; nothing else reads as
    /// an answer.
    #[test]
    fn an_answer_line_keeps_its_result_as_text() {
        let line: AnswerLine = serde_json::from_str(r#"{"id":7,"ok":null}"#).unwrap();
        assert_eq!((line.id, line.ok.map(RawValue::get)), (7, Some("null")));
        let line: AnswerLine =
            serde_json::from_str(r#"{"id":8,"ok":{"paths":["a"],"truncated":false}}"#).unwrap();
        let answer = LinkAnswer::Raw(line.ok.unwrap().to_owned());
        let decoded = match answer {
            LinkAnswer::Raw(raw) => {
                serde_json::from_str::<hide_node_link::index::Walked>(raw.get())
            }
            LinkAnswer::Parsed(_) => unreachable!(),
        }
        .unwrap();
        assert_eq!(decoded.paths, vec!["a".to_owned()]);
        let line: AnswerLine =
            serde_json::from_str(r#"{"id":9,"error":{"code":"invalid_path","message":"no"}}"#)
                .unwrap();
        assert!(line.ok.is_none());
        assert_eq!(
            line.error.unwrap().code,
            hide_node_link::ErrorCode::InvalidPath
        );
        assert!(serde_json::from_str::<AnswerLine>(r#"{"ok":1}"#).is_err());
    }

    /// A helper file is reused or started only as the account's own file no
    /// group or other account can write, and never when its mode is unknown.
    #[test]
    fn a_helper_file_others_can_write_is_not_reused_or_started() {
        let me = 501;
        let file = |uid: u32, mode: Option<u32>| FileAttributes {
            uid: Some(uid),
            permissions: mode.map(|mode| 0o100000 | mode),
            ..FileAttributes::empty()
        };
        assert!(private_file(&file(me, Some(0o700)), me));
        assert!(!private_file(&file(me, Some(0o720)), me));
        assert!(!private_file(&file(me, Some(0o702)), me));
        assert!(!private_file(&file(502, Some(0o700)), me));
        assert!(!private_file(&file(me, None), me));
        assert!(!private_file(&folder(me, 0o700), me));
    }

    /// The folders on the way to the helper root follow OpenSSH's rule: the
    /// account's or root's, and shared-writable only as a root sticky folder.
    #[test]
    fn a_helper_root_ancestor_another_account_can_change_is_refused() {
        let me = 501;
        assert!(validate_ancestor(&folder(me, 0o700), me, "/tmp/mine").is_ok());
        assert!(validate_ancestor(&folder(0, 0o755), me, "/usr").is_ok());
        assert!(validate_ancestor(&folder(0, 0o1777), me, "/private/tmp").is_ok());
        assert!(validate_ancestor(&folder(502, 0o755), me, "/tmp/theirs").is_err());
        assert!(validate_ancestor(&folder(me, 0o777), me, "/tmp/open").is_err());
        assert!(validate_ancestor(&folder(0, 0o777), me, "/shared").is_err());
        // A mode the device did not report is refused, even for root's.
        let unreported = FileAttributes {
            uid: Some(0),
            permissions: None,
            ..FileAttributes::empty()
        };
        assert!(validate_ancestor(&unreported, me, "/").is_err());
        // A group-writable home folder names the fix.
        let refusal = validate_ancestor(&folder(me, 0o775), me, "/home/me/.local")
            .unwrap_err()
            .to_string();
        assert!(refusal.contains("chmod go-w /home/me/.local"), "{refusal}");
        // The resolved path is checked from `/`, home and its parents too.
        assert_eq!(
            ancestors("/Users/example/.hide/host-helper").unwrap(),
            ["/", "/Users", "/Users/example", "/Users/example/.hide"]
        );
        assert_eq!(ancestors("/helper").unwrap(), ["/"]);
        assert!(ancestors("relative/helper").is_err());
        assert!(ancestors("/a/../b").is_err());
        assert!(owned_by(&folder(0, 0o755), me) && !owned_by(&folder(502, 0o755), me));
        // Component-wise: `/home/al` is not a prefix of `/home/alice`.
        assert!(Path::new("/home/alice/x").strip_prefix("/home/al").is_err());
    }

    #[test]
    fn device_platforms_map_to_package_names() {
        assert_eq!(
            platform_of("Darwin arm64").unwrap(),
            ("macos".to_owned(), "aarch64".to_owned())
        );
        assert_eq!(
            platform_of("Darwin x86_64\n").unwrap(),
            ("macos".to_owned(), "x86_64".to_owned())
        );
        assert_eq!(
            platform_of("Linux aarch64").unwrap(),
            ("linux".to_owned(), "aarch64".to_owned())
        );
        assert!(platform_of("FreeBSD amd64").is_err());
        assert!(platform_of("").is_err());
    }

    #[test]
    fn a_package_for_another_platform_is_never_substituted() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(HELPER_NAME), b"own").unwrap();
        let packages = HelperPackages::new(Some(directory.path().to_path_buf()));
        let own = packages
            .find(std::env::consts::OS, std::env::consts::ARCH)
            .unwrap();
        assert_eq!(own, directory.path().join(HELPER_NAME));
        let other_arch = if std::env::consts::ARCH == "x86_64" {
            "aarch64"
        } else {
            "x86_64"
        };
        let refused = packages.find("macos", other_arch).unwrap_err();
        assert!(
            refused.contains("does not include the device helper"),
            "{refused}"
        );
        std::fs::write(
            directory
                .path()
                .join(format!("{HELPER_NAME}-macos-{other_arch}")),
            b"x",
        )
        .unwrap();
        assert!(packages.find("macos", other_arch).is_ok());
    }

    #[cfg(unix)]
    fn write(path: &Path, bytes: &[u8], mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[cfg(unix)]
    fn relative(payload: &Payload) -> Vec<(String, bool)> {
        let mut files = payload
            .files
            .iter()
            .map(|file| (file.relative.clone(), file.executable))
            .collect::<Vec<_>>();
        files.sort();
        files
    }

    /// The kit's parts follow the helper's rule, each on its own: a build
    /// that lacks one still installs the helper and names what it left out.
    // A payload is made only where files have modes (`read_package`).
    #[cfg(unix)]
    #[test]
    fn a_device_payload_carries_every_kit_part_the_build_has() {
        let directory = tempfile::tempdir().unwrap();
        let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
        write(&directory.path().join(HELPER_NAME), b"helper", 0o755);
        let packages = HelperPackages::new(Some(directory.path().to_path_buf()));
        let bare = packages.payload(os, arch).unwrap();
        assert_eq!(bare.files[0].relative, HELPER_NAME);
        assert_eq!(bare.files.len(), 1);
        assert_eq!(bare.missing.len(), 2, "{:?}", bare.missing);

        write(&directory.path().join(CLI_NAME), b"cli", 0o755);
        write(&directory.path().join(HOOKS_NAME), b"hooks", 0o755);
        let full = packages.payload(os, arch).unwrap();
        assert!(full.missing.is_empty(), "{:?}", full.missing);
        assert_eq!(
            relative(&full),
            vec![
                ("hide".to_owned(), true),
                ("hide-agent-hooks".to_owned(), true),
                ("hided".to_owned(), true),
            ]
        );
        assert_ne!(full.version(), bare.version());

        // Another processor gets its own helper,
        // never this machine's binaries.
        let other_arch = if arch == "x86_64" {
            "aarch64"
        } else {
            "x86_64"
        };
        write(
            &directory
                .path()
                .join(format!("{HELPER_NAME}-{os}-{other_arch}")),
            b"other",
            0o755,
        );
        let other = packages.payload(os, other_arch).unwrap();
        assert_eq!(relative(&other), vec![("hided".to_owned(), true),]);
        assert_eq!(other.missing.len(), 2, "{:?}", other.missing);
    }

    /// Any change to any part is a new version folder on the device, so a
    /// hook never runs a mix of two builds.
    #[cfg(unix)]
    #[test]
    fn the_payload_version_follows_every_file_and_mode() {
        let directory = tempfile::tempdir().unwrap();
        let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
        write(&directory.path().join(HELPER_NAME), b"helper", 0o755);
        write(&directory.path().join(HOOKS_NAME), b"one", 0o644);
        let packages = HelperPackages::new(Some(directory.path().to_path_buf()));
        let first = packages.payload(os, arch).unwrap().version();
        assert_eq!(packages.payload(os, arch).unwrap().version(), first);
        write(&directory.path().join(HOOKS_NAME), b"two", 0o644);
        let second = packages.payload(os, arch).unwrap().version();
        assert_ne!(second, first);
        write(&directory.path().join(HOOKS_NAME), b"two", 0o755);
        assert_ne!(packages.payload(os, arch).unwrap().version(), second);
    }
}

/// Runs against a real device: `HERDR_TEST_SSH_ALIAS` names it and
/// `HERDR_TEST_REMOTE_FIXTURE` is a disposable folder on it holding
/// `checkout/a.txt` with `old`. The helper is installed under the fixture,
/// never under the account's own install root.
#[cfg(test)]
mod probe {
    use super::*;
    use hide_node_link::RootIdentity;
    use hide_node_link::protocol::{RootOpened, RootRef};

    #[test]
    #[ignore = "needs an authorized SSH device and a disposable fixture"]
    fn remote_host_open_save_conflict_and_close_probe() {
        let alias_name = std::env::var("HERDR_TEST_SSH_ALIAS").expect("configured SSH alias");
        let fixture = std::env::var("HERDR_TEST_REMOTE_FIXTURE").expect("remote fixture folder");
        let helper_dir =
            std::env::var("HERDR_TEST_HELPER_DIR").expect("local helper package folder");
        let home = std::env::var_os("HOME").expect("HOME");
        let alias =
            SshAlias::from_config_file(&PathBuf::from(home).join(".ssh/config"), &alias_name)
                .expect("SSH alias resolves");
        let client = RusshRemoteClient::new(alias).expect("client");
        let consent = HostConsent {
            contract: HOST_CONSENT_CONTRACT,
            helper_root: format!("{fixture}/helper"),
            cli_dir: Some(format!("{fixture}/bin")),
            granted_at_unix_ms: 1,
            identity: None,
        };
        let packages = HelperPackages::new(Some(PathBuf::from(helper_dir)));
        let (seen, closed) = std::sync::mpsc::channel::<String>();
        let established = establish(
            &client,
            &packages,
            &consent,
            &[],
            None,
            Box::new(move |reason| {
                let _ = seen.send(reason);
            }),
        )
        .expect("helper established");
        eprintln!(
            "probe: identity={} installed={} path={} platform={} {}",
            established.identity.describe(),
            established.installed,
            established.helper_path,
            established.hello.os,
            established.hello.arch
        );
        let host = established.host;
        let timeout = Duration::from_secs(20);
        let root_path = format!("{fixture}/checkout");
        let opened: RootOpened = call_as(
            &*host,
            Call::RootOpen {
                root: root_path.clone(),
            },
            timeout,
        )
        .expect("root opens");
        let root = RootRef {
            path: root_path,
            identity: opened.identity,
        };
        let listing: hide_node_link::list::Listing = call_as(
            &*host,
            Call::List {
                root: root.clone(),
                path: String::new(),
            },
            timeout,
        )
        .expect("listing");
        assert!(
            listing.entries.iter().any(|entry| entry.name == "a.txt"),
            "{listing:?}"
        );
        let document: hide_node_link::document::Document = call_as(
            &*host,
            Call::OpenDocument {
                root: root.clone(),
                path: "a.txt".to_owned(),
            },
            timeout,
        )
        .expect("document");
        assert_eq!(document.contents.as_deref(), Some("old\n"));
        let revision = document.revision.expect("editable revision");
        let saved: hide_node_link::save::Saved = call_as(
            &*host,
            Call::Save {
                root: root.clone(),
                path: "a.txt".to_owned(),
                contents: "saved from the probe\n".to_owned(),
                expected_revision: revision.clone(),
            },
            timeout,
        )
        .expect("save");
        assert_ne!(saved.revision, revision);
        match host.call(
            Call::Save {
                root: root.clone(),
                path: "a.txt".to_owned(),
                contents: "stale draft\n".to_owned(),
                expected_revision: revision,
            },
            timeout,
        ) {
            Err(LinkError::Refused(error)) => {
                assert_eq!(error.code, hide_node_link::ErrorCode::Conflict);
                assert_eq!(
                    error.actual_revision.as_deref(),
                    Some(saved.revision.as_str())
                );
            }
            other => panic!("a stale save must be a conflict: {other:?}"),
        }
        match host.call(
            Call::List {
                root: RootRef {
                    path: root.path.clone(),
                    identity: RootIdentity {
                        device: 0,
                        inode: 0,
                    },
                },
                path: String::new(),
            },
            timeout,
        ) {
            Err(LinkError::Refused(error)) => {
                assert_eq!(error.code, hide_node_link::ErrorCode::RootReplaced)
            }
            other => panic!("a wrong root identity must be refused: {other:?}"),
        }
        match host.call(
            Call::OpenDocument {
                root: root.clone(),
                path: "../escape.txt".to_owned(),
            },
            timeout,
        ) {
            Err(LinkError::Refused(error)) => {
                assert_eq!(error.code, hide_node_link::ErrorCode::InvalidPath)
            }
            other => panic!("a traversal must be refused: {other:?}"),
        }
        host.close("probe finished");
        match host.call(Call::Hello, timeout) {
            Err(LinkError::NotConnected(_)) => {}
            other => panic!("a closed host takes no request: {other:?}"),
        }
        eprintln!(
            "probe: close observed = {:?}",
            closed.recv_timeout(Duration::from_secs(10)).ok()
        );
    }
}
