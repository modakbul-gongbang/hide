//! A device's file host: `hide-host-helper` serving the `hide_host` contract
//! over one SSH exec channel (PRD S5.5 D-05, D-20, D-23).
//!
//! Nothing here runs without the operator's consent for that device, recorded
//! on its registration and bound to the SSH identity the helper was first
//! allowed on. The helper is installed (or replaced by a newer build) under the
//! consented install root, started with `serve` on an exec channel of a
//! dedicated SSH connection, and ends when that connection does: there is no
//! daemon, no listening socket and no background install.
//!
//! Admission is bounded per device: four requests run and thirty-two wait; a
//! request past that is refused as busy, never dropped. A request that was
//! sent and got no answer, because the connection ended or the deadline
//! passed, is `Unknown`: it may have taken effect, and the caller settles it
//! by reading the target again rather than by resending it (B14, B33).

use super::*;
pub use crate::host_access::HostCallError;
use crate::host_access::{HostAnswer, HostChannel, call_as};
use crate::model::{HostConsent, HostIdentity};
use hide_host::HostError;
use hide_host::protocol::{Call, Hello, PROTOCOL_VERSION, Request};
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

/// One answer line as the helper sends it (`hide_host::protocol::Response`),
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

/// The scope the operator agrees to, versioned. A build that needs more than
/// this contract describes bumps it, and every device asks again (B51).
pub const HOST_CONSENT_CONTRACT: u32 = 1;

/// Where the helper is installed on the device unless the daemon was started
/// with another root; `~` is the remote account's home.
pub const DEFAULT_HELPER_ROOT: &str = "~/.local/share/hide/host-helper";

pub const MAX_RUNNING: usize = hide_host::serve::CONCURRENCY;
pub const MAX_QUEUED: usize = 32;

const HELPER_NAME: &str = "hide-host-helper";
const INSTALL_TIMEOUT: Duration = Duration::from_secs(120);
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
    /// `hide-host-helper-<os>-<arch>` file wins; an unsuffixed
    /// `hide-host-helper` is accepted only for this daemon's own platform,
    /// which is what a development build produces.
    pub fn find(&self, os: &str, arch: &str) -> Result<PathBuf, String> {
        let directory = self.directory.as_ref().ok_or_else(|| {
            "This Hide has no helper packages, so files and Git on devices are unavailable"
                .to_owned()
        })?;
        let named = directory.join(format!("{HELPER_NAME}-{os}-{arch}"));
        if named.is_file() {
            return Ok(named);
        }
        let own = directory.join(HELPER_NAME);
        if os == std::env::consts::OS && arch == std::env::consts::ARCH && own.is_file() {
            return Ok(own);
        }
        Err(format!(
            "This Hide build does not include the device helper for {} {arch}; files and Git on this device stay unavailable until a build that packages it is installed",
            platform_label(os)
        ))
    }
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

#[derive(Debug)]
pub enum EstablishError {
    Connect(RemoteError),
    /// The device answering the alias is not the one consent was given for.
    IdentityChanged {
        bound: Box<HostIdentity>,
        observed: Box<HostIdentity>,
    },
    /// This build cannot serve the device; nothing was installed.
    Unsupported(String),
    Install(String),
    Helper(String),
}

impl fmt::Display for EstablishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(error) => write!(formatter, "{}", error.diagnostic().reason),
            Self::IdentityChanged { bound, observed } => write!(
                formatter,
                "The device now answers as {}, not {} that the helper was allowed on; allow it again in Settings to continue",
                observed.describe(),
                bound.describe()
            ),
            Self::Unsupported(reason) | Self::Install(reason) | Self::Helper(reason) => {
                formatter.write_str(reason)
            }
        }
    }
}

pub struct Established {
    pub host: RemoteHost,
    pub identity: HostIdentity,
    pub hello: Hello,
    /// The helper was installed or replaced on this connection.
    pub installed: bool,
    pub helper_path: String,
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

struct Admission {
    running: usize,
    queued: usize,
}

struct Inner {
    target: String,
    runtime: Arc<Runtime>,
    session: Mutex<Option<Handle<KnownHostHandler>>>,
    writer: tokio::sync::Mutex<Pin<Box<dyn AsyncWrite + Send>>>,
    pending: Mutex<HashMap<u64, mpsc::Sender<Answered>>>,
    closed: Mutex<Option<String>>,
    admission: Mutex<Admission>,
    admitted: Condvar,
    next_id: AtomicU64,
    /// Checkout roots this connection has opened, pinned to the directory
    /// they named then.
    roots: Mutex<HashMap<String, hide_host::RootIdentity>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(session) = lock_recover(&self.session).take() {
            self.runtime.spawn(async move {
                let _ = session
                    .disconnect(Disconnect::ByApplication, "helper closed", "en")
                    .await;
            });
        }
    }
}

impl RemoteHost {
    pub fn target(&self) -> &str {
        &self.inner.target
    }

    /// Why the connection ended, once it has.
    pub fn closed_reason(&self) -> Option<String> {
        lock_recover(&self.inner.closed).clone()
    }

    /// Ends the connection; the helper exits when its input closes. Requests
    /// still waiting for an answer become `Unknown`.
    pub fn close(&self, reason: &str) {
        mark_closed(&self.inner, reason.to_owned());
        if let Some(session) = lock_recover(&self.inner.session).take() {
            let reason = reason.to_owned();
            self.inner.runtime.spawn(async move {
                let _ = session
                    .disconnect(Disconnect::ByApplication, &reason, "en")
                    .await;
            });
        }
    }

    /// Sends one request and waits at most `timeout` for its answer. Must
    /// not be called from inside an async context.
    pub fn call(&self, call: Call, timeout: Duration) -> Result<HostAnswer, HostCallError> {
        let inner = &self.inner;
        self.admit(timeout)?;
        let result = self.send_and_wait(call, timeout);
        let mut admission = lock_recover(&inner.admission);
        admission.running -= 1;
        drop(admission);
        inner.admitted.notify_one();
        result
    }

    fn admit(&self, timeout: Duration) -> Result<(), HostCallError> {
        let inner = &self.inner;
        if let Some(reason) = self.closed_reason() {
            return Err(HostCallError::NotConnected(reason));
        }
        let mut admission = lock_recover(&inner.admission);
        if admission.running < MAX_RUNNING {
            admission.running += 1;
            return Ok(());
        }
        if admission.queued >= MAX_QUEUED {
            return Err(HostCallError::Busy);
        }
        admission.queued += 1;
        let deadline = Instant::now() + timeout;
        while admission.running >= MAX_RUNNING {
            let now = Instant::now();
            if now >= deadline {
                admission.queued -= 1;
                return Err(HostCallError::Busy);
            }
            admission = inner
                .admitted
                .wait_timeout(admission, deadline - now)
                .map(|(guard, _)| guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner().0);
        }
        admission.queued -= 1;
        admission.running += 1;
        Ok(())
    }

    fn send_and_wait(&self, call: Call, timeout: Duration) -> Result<HostAnswer, HostCallError> {
        let inner = &self.inner;
        let id = inner.next_id.fetch_add(1, Ordering::Relaxed);
        let mut line = serde_json::to_vec(&Request { id, call }).map_err(|error| {
            HostCallError::NotConnected(format!("The request could not be encoded: {error}"))
        })?;
        line.push(b'\n');
        let (sender, receiver) = mpsc::channel();
        lock_recover(&inner.pending).insert(id, sender);
        if let Some(reason) = self.closed_reason() {
            lock_recover(&inner.pending).remove(&id);
            return Err(HostCallError::NotConnected(reason));
        }
        // Waiting behind another request's write sends nothing, so running
        // out of time there leaves the connection as it was; only a write
        // that started and did not finish can have sent part of a line.
        let deadline = tokio::time::Instant::now() + timeout;
        let written = inner.runtime.block_on(async {
            let Ok(mut writer) = tokio::time::timeout_at(deadline, inner.writer.lock()).await
            else {
                return None;
            };
            Some(
                tokio::time::timeout_at(deadline, async {
                    writer.write_all(&line).await?;
                    writer.flush().await
                })
                .await,
            )
        });
        let Some(written) = written else {
            lock_recover(&inner.pending).remove(&id);
            return Err(HostCallError::Busy);
        };
        match written {
            Ok(Ok(())) => {}
            // A write that failed or timed out may have sent part of the
            // line, and the next request would be read as its tail, so the
            // connection ends here; the next use reconnects.
            Ok(Err(error)) => {
                lock_recover(&inner.pending).remove(&id);
                self.close("a request could not be written to the device helper");
                return Err(HostCallError::Unknown(format!(
                    "The connection to the device failed while the request was sent ({error}); its result is unknown"
                )));
            }
            Err(_) => {
                lock_recover(&inner.pending).remove(&id);
                self.close("a request was not accepted by the device helper in time");
                return Err(HostCallError::Unknown(
                    "The device did not accept the request in time; its result is unknown"
                        .to_owned(),
                ));
            }
        }
        match receiver.recv_timeout(timeout) {
            Ok(Ok(raw)) => Ok(HostAnswer::Raw(raw)),
            Ok(Err(error)) => Err(HostCallError::Refused(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                lock_recover(&inner.pending).remove(&id);
                Err(HostCallError::Unknown(
                    "The device did not answer in time; the result is unknown and nothing was resent"
                        .to_owned(),
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(HostCallError::Unknown(format!(
                "The connection to the device ended before it answered ({}); the result is unknown and nothing was resent",
                self.closed_reason()
                    .unwrap_or_else(|| "no reason reported".to_owned())
            ))),
        }
    }
}

impl HostChannel for RemoteHost {
    fn call(&self, call: Call, timeout: Duration) -> Result<HostAnswer, HostCallError> {
        RemoteHost::call(self, call, timeout)
    }

    fn close_when_idle(&self, reason: &str) {
        let host = RemoteHost {
            inner: Arc::clone(&self.inner),
        };
        let reason = reason.to_owned();
        let _ = std::thread::Builder::new()
            .name("remote-host-drain".into())
            .spawn(move || {
                // Admitted requests are bounded by their own timeouts; this
                // bound only keeps a wedged count from holding the link open.
                let deadline = std::time::Instant::now() + Duration::from_secs(120);
                loop {
                    let admission = lock_recover(&host.inner.admission);
                    if admission.running == 0 && admission.queued == 0 {
                        break;
                    }
                    drop(admission);
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                host.close(&reason);
            });
    }

    fn closed_reason(&self) -> Option<String> {
        RemoteHost::closed_reason(self)
    }

    fn close(&self, reason: &str) {
        RemoteHost::close(self, reason)
    }

    fn pinned(&self, root: &str) -> Option<hide_host::RootIdentity> {
        lock_recover(&self.inner.roots).get(root).copied()
    }

    fn pin(&self, root: &str, identity: Option<hide_host::RootIdentity>) {
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
        *closed = Some(reason);
    }
    drop(closed);
    // Dropping the senders wakes every waiting request as disconnected.
    lock_recover(&inner.pending).clear();
    inner.admitted.notify_all();
}

impl HostIdentity {
    pub fn describe(&self) -> String {
        format!(
            "{}@{}:{} ({})",
            self.user, self.hostname, self.port, self.host_key_sha256
        )
    }
}

/// Connects, checks consent against the device that answered, installs the
/// helper when the device lacks this build's, and starts it. Blocking; run
/// it off the runtime lock.
pub fn establish(
    client: &RusshRemoteClient,
    packages: &HelperPackages,
    consent: &HostConsent,
    on_close: Box<dyn FnOnce(String) + Send + 'static>,
) -> Result<Established, EstablishError> {
    let observed_key = Arc::new(Mutex::new(None));
    let handler =
        KnownHostHandler::new(&client.host, None).with_observed_key(Arc::clone(&observed_key));
    let session = client
        .runtime
        .block_on(async {
            tokio::time::timeout(SSH_OPERATION_TIMEOUT, client.connect(handler))
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
        host_key_sha256: lock_recover(&observed_key).clone().unwrap_or_default(),
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
    let mut session = session;
    let result = client.runtime.block_on(async {
        tokio::time::timeout(
            INSTALL_TIMEOUT,
            start_helper(&mut session, client, packages, &consent.helper_root),
        )
        .await
        .unwrap_or_else(|_| {
            Err(EstablishError::Install(
                "Installing the device helper timed out; nothing was started".to_owned(),
            ))
        })
    });
    let (channel, installed, helper_path) = match result {
        Ok(parts) => parts,
        Err(error) => {
            let _ = client.runtime.block_on(session.disconnect(
                Disconnect::ByApplication,
                "helper setup failed",
                "en",
            ));
            return Err(error);
        }
    };
    let host = spawn_host(client, session, channel, on_close);
    let hello: Hello = call_as(&host, Call::Hello, HELLO_TIMEOUT).map_err(|error| {
        EstablishError::Helper(format!("The device helper did not start: {error}"))
    })?;
    if hello.protocol != PROTOCOL_VERSION {
        host.close("helper protocol mismatch");
        return Err(EstablishError::Helper(format!(
            "The device helper speaks protocol {}, this Hide needs {PROTOCOL_VERSION}",
            hello.protocol
        )));
    }
    Ok(Established {
        host,
        identity,
        hello,
        installed,
        helper_path,
    })
}

async fn start_helper(
    session: &mut Handle<KnownHostHandler>,
    client: &RusshRemoteClient,
    packages: &HelperPackages,
    helper_root: &str,
) -> Result<(Channel<Msg>, bool, String), EstablishError> {
    let target = client.host.host_id.clone();
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
    let (os, arch) = platform_of(uname.stdout.trim()).map_err(EstablishError::Unsupported)?;
    let package = packages
        .find(&os, &arch)
        .map_err(EstablishError::Unsupported)?;
    let bytes = std::fs::read(&package).map_err(|error| {
        EstablishError::Install(format!(
            "The helper package {} could not be read: {error}",
            package.display()
        ))
    })?;
    let digest = hex_digest(&bytes);

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
    let installed = install(&raw, helper_root, &digest, &bytes).await;
    let _ = raw.close_session();
    let (helper_path, fresh) = installed?;

    let channel = session.channel_open_session().await.map_err(|error| {
        EstablishError::Helper(format!("The helper channel could not be opened: {error}"))
    })?;
    channel
        .exec(true, format!("{} serve", shell_quote(&helper_path)))
        .await
        .map_err(|error| {
            EstablishError::Helper(format!("The helper could not be started: {error}"))
        })?;
    Ok((channel, fresh, helper_path))
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

/// Puts this build's helper at `<root>/<digest prefix>/hide-host-helper`,
/// reusing an existing copy only after checking its bytes. Returns the path
/// and whether anything was written. Older builds under the same root are
/// removed afterwards: the root is the one the operator allowed Hide to own.
async fn install(
    raw: &RawSftpSession,
    helper_root: &str,
    digest: &str,
    bytes: &[u8],
) -> Result<(String, bool), EstablishError> {
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
    ensure_private_dirs(raw, &home, &root, owner).await?;
    let version_dir = format!("{root}/{}", &digest[..16]);
    ensure_private_dir(raw, &version_dir, owner).await?;
    let final_path = format!("{version_dir}/{HELPER_NAME}");
    if let Ok(existing) = raw.lstat(&final_path).await
        && existing.attrs.is_regular()
        && existing.attrs.uid == Some(owner)
        && existing.attrs.size == Some(bytes.len() as u64)
        && remote_digest(raw, &final_path, bytes.len())
            .await
            .ok()
            .as_deref()
            == Some(digest)
    {
        remove_older_builds(raw, &root, &digest[..16]).await;
        return Ok((final_path, false));
    }
    let staging = format!("{version_dir}/.upload-{}", std::process::id());
    let _ = raw.remove(&staging).await;
    let handle = raw
        .open(
            &staging,
            OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
            FileAttributes {
                permissions: Some(0o700),
                ..FileAttributes::empty()
            },
        )
        .await
        .map_err(|error| sftp_failure("The helper could not be uploaded", error))?
        .handle;
    let written = async {
        for (index, chunk) in bytes.chunks(32 * 1024).enumerate() {
            raw.write(&handle, (index * 32 * 1024) as u64, chunk.to_vec())
                .await
                .map_err(|error| sftp_failure("The helper upload failed", error))?;
        }
        Ok::<(), EstablishError>(())
    }
    .await;
    let closed = raw.close(handle).await;
    let verified = match (written, closed) {
        (Ok(()), Ok(_)) => match remote_digest(raw, &staging, bytes.len()).await {
            Ok(uploaded) if uploaded == digest => Ok(()),
            Ok(_) => Err(EstablishError::Install(
                "The uploaded helper did not match the package; it was not used".to_owned(),
            )),
            Err(error) => Err(error),
        },
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(sftp_failure("The helper upload did not finish", error)),
    };
    if let Err(error) = verified {
        let _ = raw.remove(&staging).await;
        return Err(error);
    }
    let _ = raw.remove(&final_path).await;
    raw.rename(&staging, &final_path)
        .await
        .map_err(|error| sftp_failure("The helper could not be put in place", error))?;
    remove_older_builds(raw, &root, &digest[..16]).await;
    Ok((final_path, true))
}

async fn ensure_private_dirs(
    raw: &RawSftpSession,
    home: &str,
    root: &str,
    owner: u32,
) -> Result<(), EstablishError> {
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
                raw.mkdir(
                    &current,
                    FileAttributes {
                        permissions: Some(0o700),
                        ..FileAttributes::empty()
                    },
                )
                .await
                .map_err(|error| sftp_failure("The helper folder could not be created", error))?;
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
    validate_private(&attrs, owner, root)
}

async fn ensure_private_dir(
    raw: &RawSftpSession,
    path: &str,
    owner: u32,
) -> Result<(), EstablishError> {
    match raw.lstat(path).await {
        Ok(attrs) => validate_private(&attrs.attrs, owner, path),
        Err(SftpError::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
            raw.mkdir(
                path,
                FileAttributes {
                    permissions: Some(0o700),
                    ..FileAttributes::empty()
                },
            )
            .await
            .map_err(|error| sftp_failure("The helper folder could not be created", error))?;
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
/// and writable by no other account unless it is a root-owned sticky folder
/// such as `/tmp`, where others cannot rename what the account made.
fn validate_ancestor(attrs: &FileAttributes, owner: u32, path: &str) -> Result<(), EstablishError> {
    let mode = attrs.permissions.unwrap_or(0o7777);
    let shared = mode & 0o022 != 0;
    let root_sticky = attrs.uid == Some(0) && mode & 0o1000 != 0;
    if owned_by(attrs, owner) && (!shared || root_sticky) {
        return Ok(());
    }
    Err(EstablishError::Install(format!(
        "{path} can be changed by another account, so the helper was not installed under it"
    )))
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
) -> Result<String, EstablishError> {
    let handle = raw
        .open(path, OpenFlags::READ, FileAttributes::empty())
        .await
        .map_err(|error| sftp_failure("The helper could not be read back", error))?
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
                .map_err(|error| sftp_failure("The helper could not be read back", error))?
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
        return Err(EstablishError::Install(
            "The helper on the device is incomplete".to_owned(),
        ));
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Removes `<root>/<16 hex>/hide-host-helper` builds other than `keep`.
/// Only names this installer creates are touched; anything else stays.
async fn remove_older_builds(raw: &RawSftpSession, root: &str, keep: &str) {
    let Ok(directory) = raw.opendir(root).await else {
        return;
    };
    let mut names = Vec::new();
    while let Ok(listing) = raw.readdir(&directory.handle).await {
        if listing.files.is_empty() {
            break;
        }
        for entry in listing.files {
            let name = entry.filename;
            if name.len() == 16 && name.bytes().all(|byte| byte.is_ascii_hexdigit()) && name != keep
            {
                names.push(name);
            }
        }
    }
    let _ = raw.close(directory.handle).await;
    for name in names {
        let folder = format!("{root}/{name}");
        let helper = format!("{folder}/{HELPER_NAME}");
        if raw.remove(&helper).await.is_ok() {
            let _ = raw.rmdir(&folder).await;
        }
    }
}

fn spawn_host(
    client: &RusshRemoteClient,
    session: Handle<KnownHostHandler>,
    channel: Channel<Msg>,
    on_close: Box<dyn FnOnce(String) + Send + 'static>,
) -> RemoteHost {
    let writer: Pin<Box<dyn AsyncWrite + Send>> = Box::pin(channel.make_writer());
    let inner = Arc::new(Inner {
        target: client.host.host_id.clone(),
        runtime: Arc::clone(&client.runtime),
        session: Mutex::new(Some(session)),
        writer: tokio::sync::Mutex::new(writer),
        pending: Mutex::new(HashMap::new()),
        closed: Mutex::new(None),
        admission: Mutex::new(Admission {
            running: 0,
            queued: 0,
        }),
        admitted: Condvar::new(),
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
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    buffer.extend_from_slice(&data);
                    while let Some(offset) = buffer[scanned..].iter().position(|byte| *byte == b'\n') {
                        let end = scanned + offset;
                        scanned = 0;
                        let line: Vec<u8> = buffer.drain(..=end).collect();
                        let Some(inner) = reader.upgrade() else { return };
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
        if let Some(inner) = reader.upgrade() {
            crate::diagnostic!(json!({
                "component": "remote_host",
                "kind": "host.closed",
                "target": inner.target,
                "reason": reason,
                "stderr_tail": String::from_utf8_lossy(&stderr).chars().rev().take(400).collect::<String>().chars().rev().collect::<String>(),
            }));
            mark_closed(&inner, reason.clone());
        }
        on_close(reason);
    });
    RemoteHost { inner }
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

    /// An answer line keeps its result as the helper's own text, `null`
    /// included, and a refusal as the helper's error; nothing else reads as
    /// an answer.
    #[test]
    fn an_answer_line_keeps_its_result_as_text() {
        let line: AnswerLine = serde_json::from_str(r#"{"id":7,"ok":null}"#).unwrap();
        assert_eq!((line.id, line.ok.map(RawValue::get)), (7, Some("null")));
        let line: AnswerLine =
            serde_json::from_str(r#"{"id":8,"ok":{"paths":["a"],"truncated":false}}"#).unwrap();
        let answer = HostAnswer::Raw(line.ok.unwrap().to_owned());
        let decoded = match answer {
            HostAnswer::Raw(raw) => serde_json::from_str::<hide_host::index::Walked>(raw.get()),
            HostAnswer::Parsed(_) => unreachable!(),
        }
        .unwrap();
        assert_eq!(decoded.paths, vec!["a".to_owned()]);
        let line: AnswerLine =
            serde_json::from_str(r#"{"id":9,"error":{"code":"invalid_path","message":"no"}}"#)
                .unwrap();
        assert!(line.ok.is_none());
        assert_eq!(line.error.unwrap().code, hide_host::ErrorCode::InvalidPath);
        assert!(serde_json::from_str::<AnswerLine>(r#"{"ok":1}"#).is_err());
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
}

/// Runs against a real device: `HERDR_TEST_SSH_ALIAS` names it and
/// `HERDR_TEST_REMOTE_FIXTURE` is a disposable folder on it holding
/// `checkout/a.txt` with `old`. The helper is installed under the fixture,
/// never under the account's own install root.
#[cfg(test)]
mod probe {
    use super::*;
    use hide_host::RootIdentity;
    use hide_host::protocol::{RootOpened, RootRef};

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
            granted_at_unix_ms: 1,
            identity: None,
        };
        let packages = HelperPackages::new(Some(PathBuf::from(helper_dir)));
        let closed = Arc::new(Mutex::new(None::<String>));
        let seen = Arc::clone(&closed);
        let established = establish(
            &client,
            &packages,
            &consent,
            Box::new(move |reason| {
                *lock_recover(&seen) = Some(reason);
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
            &host,
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
        let listing: hide_host::list::Listing = call_as(
            &host,
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
        let document: hide_host::document::Document = call_as(
            &host,
            Call::OpenDocument {
                root: root.clone(),
                path: "a.txt".to_owned(),
            },
            timeout,
        )
        .expect("document");
        assert_eq!(document.contents.as_deref(), Some("old\n"));
        let revision = document.revision.expect("editable revision");
        let saved: hide_host::save::Saved = call_as(
            &host,
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
            Err(HostCallError::Refused(error)) => {
                assert_eq!(error.code, hide_host::ErrorCode::Conflict);
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
            Err(HostCallError::Refused(error)) => {
                assert_eq!(error.code, hide_host::ErrorCode::RootReplaced)
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
            Err(HostCallError::Refused(error)) => {
                assert_eq!(error.code, hide_host::ErrorCode::InvalidPath)
            }
            other => panic!("a traversal must be refused: {other:?}"),
        }
        host.close("probe finished");
        match host.call(Call::Hello, timeout) {
            Err(HostCallError::NotConnected(_)) => {}
            other => panic!("a closed host takes no request: {other:?}"),
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while lock_recover(&closed).is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        eprintln!("probe: close observed = {:?}", lock_recover(&closed));
    }
}
