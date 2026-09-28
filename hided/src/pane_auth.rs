//! Bootstrap of short-lived Workspace credentials. This local socket does
//! only kernel peer attestation and credential issuance; commands use `/ws`.
//!
//! A caller is bound one of two ways. A process descending from the shell
//! Herdr runs for the named pane gets a pane-bound capability that records
//! the pane's terminal identity and shell birth and dies with the pane. A
//! local process that is not such a descendant (a Codex tool shell run by
//! the shared `codex app-server` daemon, whose parent is launchd) is bound
//! instead to the registered, connected checkout that contains its working
//! directory, read from the kernel for the peer pid and never from the
//! request; that capability is rechecked only for registration and device
//! connection. Both hold the same Workspace commands for the same checkout.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use herdr_core::remote::RusshRemoteClient;
use herdr_core::workspace_control::{Caller, Context, Query, checkout_caller_id};
use hide_herdr_client::{UnixSocketConnector, request_with_connector};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::UnixListener;
use tokio::sync::{Notify, Semaphore};

use crate::core::CoreHandle;
use crate::state_file::new_token;

const MAX_CAPABILITIES: usize = 64;
const CAPABILITY_LIFETIME: Duration = Duration::from_secs(8 * 60 * 60);
/// What a caller can do when its checkout no longer resolves: the same
/// sentence at bootstrap (CLI), at a refused command (server), and in the
/// caller's own refusal text.
pub const CHECKOUT_NEXT_ACTION: &str = "Run the command from a shell inside a registered project checkout, or reconnect Hide, and retry";
pub const PANE_NEXT_ACTION: &str = "Reconnect the pane and retry";
const UNCLAIMED_LIFETIME: Duration = Duration::from_secs(30);
const HERDR_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PARENT_HOPS: usize = 32;
const MAX_BOOTSTRAPS: usize = 8;
const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(5);

/// How a capability was bound to its caller, and what its validation rechecks.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Binding {
    Pane {
        terminal_id: String,
        shell_pid: i32,
        shell_started: u64,
    },
    Checkout,
}

#[derive(Clone)]
pub struct Capability {
    /// The caller id the core resolves: a pane id, or the encoded checkout
    /// caller (`workspace_control::Caller`) for a checkout-bound capability.
    pub pane_id: String,
    pub context: Context,
    binding: Binding,
    pub one_shot: bool,
    holder: Option<(i32, u64)>,
    created: Instant,
    claimed: bool,
    path: PathBuf,
    remote: Option<RemoteCapability>,
}

impl Capability {
    /// The refusal for a command whose answer no longer matches this
    /// credential's context: a pane moved or closed, or a checkout that is
    /// no longer registered on its device.
    pub fn changed_refusal(&self) -> (&'static str, &'static str) {
        match self.binding {
            Binding::Pane { .. } => ("pane_changed", PANE_NEXT_ACTION),
            Binding::Checkout => ("checkout_not_registered", CHECKOUT_NEXT_ACTION),
        }
    }
}

/// The next step for a refused command, by the daemon's reason.
pub fn refusal_next_action(reason: &str) -> &'static str {
    match reason {
        "checkout_not_registered" | "caller_unavailable" => CHECKOUT_NEXT_ACTION,
        _ => PANE_NEXT_ACTION,
    }
}

#[derive(Clone)]
struct RemoteCapability {
    bridge_id: String,
    alive: Arc<AtomicBool>,
    client: Arc<RusshRemoteClient>,
    helper_path: String,
    source_pane_id: String,
}

pub(crate) struct RemoteGrant {
    pub bridge_id: String,
    pub alive: Arc<AtomicBool>,
    pub client: Arc<RusshRemoteClient>,
    pub helper_path: String,
    pub source_pane_id: String,
    pub one_shot: bool,
}

fn entry_alive(entry: &Capability) -> bool {
    if entry.created.elapsed() >= CAPABILITY_LIFETIME
        || (!entry.claimed && entry.created.elapsed() >= UNCLAIMED_LIFETIME)
    {
        return false;
    }
    if let Some(remote) = &entry.remote {
        return remote.alive.load(Ordering::Acquire);
    }
    let shell_alive = match &entry.binding {
        Binding::Pane {
            shell_pid,
            shell_started,
            ..
        } => process_start(*shell_pid) == Some(*shell_started),
        Binding::Checkout => true,
    };
    entry.path.is_file()
        && shell_alive
        && entry
            .holder
            .is_none_or(|(pid, born)| process_start(pid) == Some(born))
}

/// A persistent bootstrap from the same caller reuses its live capability. A
/// pane caller is the same pane with the same shell; a checkout caller is any
/// caller bound to the same checkout, whatever nonce its bootstrap carried.
fn same_caller(entry: &Capability, attestation: &Attestation) -> bool {
    entry.context == attestation.context
        && entry.binding == attestation.binding
        && match attestation.binding {
            Binding::Pane { .. } => entry.pane_id == attestation.pane_id,
            Binding::Checkout => true,
        }
}

fn remove_local_reference(entry: &Capability) {
    if entry.remote.is_none() {
        let _ = fs::remove_file(&entry.path);
        let _ = fs::remove_file(entry.path.with_extension("claimed"));
    }
}

#[derive(Serialize, Deserialize)]
pub struct Reference {
    pub token: String,
    pub port: u16,
    pub origin_port: u16,
}

pub struct Registry {
    directory: PathBuf,
    entries: Mutex<HashMap<String, Capability>>,
    closed: AtomicBool,
}

impl Registry {
    pub fn new(state_dir: &Path) -> Result<Self, String> {
        let directory = state_dir.join("pane-capabilities");
        if let Ok(metadata) = fs::symlink_metadata(&directory)
            && (!metadata.is_dir() || metadata.file_type().is_symlink())
        {
            return Err("pane capability directory is not a directory".to_owned());
        }
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        // A daemon restart invalidates every earlier token. Remove the old
        // references before the new daemon accepts a caller.
        for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            if entry.file_name().to_string_lossy().ends_with(".json") {
                fs::remove_file(entry.path()).map_err(|error| error.to_string())?;
            }
        }
        Ok(Self {
            directory,
            entries: Mutex::new(HashMap::new()),
            closed: AtomicBool::new(false),
        })
    }

    pub fn reference_path(&self, nonce: &str) -> Result<PathBuf, &'static str> {
        if nonce.len() != 32 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("invalid_nonce");
        }
        Ok(self.directory.join(format!("{nonce}.json")))
    }

    pub fn issue(
        &self,
        attestation: &Attestation,
        nonce: &str,
        port: u16,
        holder: Option<(i32, u64)>,
    ) -> Result<(PathBuf, bool), &'static str> {
        let path = self.reference_path(nonce)?;
        let mut entries = self.entries.lock().map_err(|_| "capability_unavailable")?;
        if self.closed.load(Ordering::SeqCst) {
            return Err("hide_unavailable");
        }
        entries.retain(|_, entry| {
            let alive = entry_alive(entry);
            if !alive {
                remove_local_reference(entry);
            }
            alive
        });
        if holder.is_none()
            && let Some(existing) = entries.values().find(|entry| {
                !entry.one_shot && entry.remote.is_none() && same_caller(entry, attestation)
            })
        {
            return Ok((existing.path.clone(), false));
        }
        if entries.len() >= MAX_CAPABILITIES {
            return Err("capability_limit");
        }
        let token = new_token();
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|_| "reference_unavailable")?;
        let bytes = serde_json::to_vec(&Reference {
            token: token.clone(),
            port,
            origin_port: port,
        })
        .map_err(|_| "reference_unavailable")?;
        if file
            .write_all(&bytes)
            .and_then(|_| file.sync_all())
            .is_err()
        {
            let _ = fs::remove_file(&path);
            return Err("reference_unavailable");
        }
        entries.insert(
            token.clone(),
            Capability {
                pane_id: attestation.pane_id.clone(),
                context: attestation.context.clone(),
                binding: attestation.binding.clone(),
                one_shot: holder.is_some(),
                holder,
                created: Instant::now(),
                claimed: false,
                path: path.clone(),
                remote: None,
            },
        );
        Ok((path, true))
    }

    pub(crate) fn issue_remote(
        &self,
        attestation: &Attestation,
        grant: RemoteGrant,
    ) -> Result<(String, bool), &'static str> {
        let mut entries = self.entries.lock().map_err(|_| "capability_unavailable")?;
        if self.closed.load(Ordering::SeqCst) || !grant.alive.load(Ordering::Acquire) {
            return Err("hide_unavailable");
        }
        entries.retain(|_, entry| {
            let alive = entry_alive(entry);
            if !alive {
                remove_local_reference(entry);
            }
            alive
        });
        if !grant.one_shot
            && let Some((token, _)) = entries.iter().find(|(_, entry)| {
                !entry.one_shot
                    && same_caller(entry, attestation)
                    && entry
                        .remote
                        .as_ref()
                        .is_some_and(|remote| remote.bridge_id == grant.bridge_id)
            })
        {
            return Ok((token.clone(), false));
        }
        if entries.len() >= MAX_CAPABILITIES {
            return Err("capability_limit");
        }
        let token = new_token();
        entries.insert(
            token.clone(),
            Capability {
                pane_id: attestation.pane_id.clone(),
                context: attestation.context.clone(),
                binding: attestation.binding.clone(),
                one_shot: grant.one_shot,
                holder: None,
                created: Instant::now(),
                claimed: false,
                path: PathBuf::new(),
                remote: Some(RemoteCapability {
                    bridge_id: grant.bridge_id,
                    alive: grant.alive,
                    client: grant.client,
                    helper_path: grant.helper_path,
                    source_pane_id: grant.source_pane_id,
                }),
            },
        );
        Ok((token, true))
    }

    pub fn get(&self, token: &str) -> Option<Capability> {
        let mut entries = self.entries.lock().ok()?;
        if self.closed.load(Ordering::SeqCst) {
            return None;
        }
        let cap = entries.get(token)?.clone();
        if !entry_alive(&cap) {
            entries.remove(token);
            remove_local_reference(&cap);
            return None;
        }
        Some(cap)
    }

    pub fn revoke(&self, token: &str) {
        if let Ok(mut entries) = self.entries.lock()
            && let Some(entry) = entries.remove(token)
        {
            remove_local_reference(&entry);
        }
    }

    pub fn revoke_bridge_token(&self, bridge_id: &str, token: &str) {
        if let Ok(mut entries) = self.entries.lock()
            && entries
                .get(token)
                .and_then(|entry| entry.remote.as_ref())
                .is_some_and(|remote| remote.bridge_id == bridge_id)
        {
            entries.remove(token);
        }
    }

    pub fn claim(&self, token: &str) -> Result<(), &'static str> {
        let mut entries = self.entries.lock().map_err(|_| "capability_unavailable")?;
        let entry = entries.get_mut(token).ok_or("credential_expired")?;
        if !entry_alive(entry) {
            let stale = entries.remove(token).expect("entry exists");
            remove_local_reference(&stale);
            return Err("credential_expired");
        }
        entry.claimed = true;
        Ok(())
    }

    pub fn validate(
        &self,
        token: &str,
        herdr_socket: Option<&Path>,
        core: &CoreHandle,
    ) -> Result<Capability, &'static str> {
        let cap = self.get(token).ok_or("credential_expired")?;
        if cap.binding == Binding::Checkout {
            // Only registration and connection are rechecked: the checkout
            // must still resolve, on the same device, to the same context.
            let outcome =
                match core.workspace_query(&cap.context.device_id, &cap.pane_id, Query::Info) {
                    Ok(actual) if actual.context == cap.context => return Ok(cap),
                    Ok(_) => "checkout_not_registered",
                    Err(refusal) => refusal.reason,
                };
            self.revoke(token);
            note_checkout_capability(
                "checkout_capability.refused",
                "validate",
                None,
                Some(!cap.one_shot),
                Some(&cap.context),
                Some(outcome),
                None,
            );
            return Err(outcome);
        }
        let actual = if let Some(remote) = &cap.remote {
            let identity = remote
                .client
                .workspace_pane_identity(&remote.helper_path, &remote.source_pane_id)
                .map_err(|_| "remote_unavailable")?;
            let context = core
                .workspace_query(&cap.context.device_id, &cap.pane_id, Query::Info)
                .map_err(|_| "pane_not_connected")?
                .context;
            Attestation {
                pane_id: cap.pane_id.clone(),
                context,
                binding: Binding::Pane {
                    terminal_id: identity.terminal_id,
                    shell_pid: identity.shell_pid,
                    shell_started: identity.shell_started,
                },
            }
        } else {
            let socket = herdr_socket.ok_or("pane_unavailable")?;
            inspect_pane(&cap.pane_id, socket, core).inspect_err(|_| self.revoke(token))?
        };
        if actual.context != cap.context || actual.binding != cap.binding {
            self.revoke(token);
            return Err("pane_changed");
        }
        Ok(cap)
    }

    pub fn revoke_bridge(&self, bridge_id: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|_, entry| {
                entry
                    .remote
                    .as_ref()
                    .is_none_or(|remote| remote.bridge_id != bridge_id)
            });
        }
    }

    pub fn revoke_all(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            self.closed.store(true, Ordering::SeqCst);
            for entry in entries.values() {
                remove_local_reference(entry);
            }
            entries.clear();
        }
    }

    /// Drops expired entries, and checkout-bound entries whose checkout no
    /// longer resolves on its device, so an unregistered project stops
    /// counting against the 64-reference cap before its lifetime ends.
    /// The core is asked with the lock released, so a bootstrap or a
    /// validation never waits behind the sweep's round trips.
    pub fn sweep(&self, core: &CoreHandle) {
        let checkouts: Vec<(String, String, String, Context)> = match self.entries.lock() {
            Ok(entries) => entries
                .iter()
                .filter(|(_, entry)| entry.binding == Binding::Checkout && entry_alive(entry))
                .map(|(token, entry)| {
                    (
                        token.clone(),
                        entry.context.device_id.clone(),
                        entry.pane_id.clone(),
                        entry.context.clone(),
                    )
                })
                .collect(),
            Err(_) => return,
        };
        let gone: HashMap<String, &'static str> = checkouts
            .into_iter()
            .filter_map(|(token, device_id, caller_id, context)| {
                match core.workspace_query(&device_id, &caller_id, Query::Info) {
                    Ok(actual) if actual.context == context => None,
                    Ok(_) => Some((token, "checkout_not_registered")),
                    Err(refusal) => Some((token, refusal.reason)),
                }
            })
            .collect();
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|token, entry| {
                let mut alive = entry_alive(entry);
                if let (true, Some(reason)) = (alive, gone.get(token)) {
                    note_checkout_capability(
                        "checkout_capability.refused",
                        "sweep",
                        None,
                        Some(!entry.one_shot),
                        Some(&entry.context),
                        Some(reason),
                        None,
                    );
                    alive = false;
                }
                if !alive {
                    remove_local_reference(entry);
                }
                alive
            });
        }
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        self.revoke_all();
    }
}

/// A bootstrap request has no authority until the kernel peer PID is shown
/// to be a descendant of the shell Herdr owns for this pane, or, failing
/// that, to have its working directory inside a registered checkout. The
/// pane id may be absent for a caller that has none.
#[derive(Deserialize)]
pub struct BootstrapRequest {
    #[serde(default)]
    pub pane_id: String,
    pub nonce: String,
    #[serde(default)]
    pub one_shot: bool,
}

#[derive(Clone, Debug)]
pub struct Attestation {
    pane_id: String,
    context: Context,
    binding: Binding,
}

/// The bootstrap's attestation. The pane path runs unchanged first; any
/// pane failure, including a Herdr socket hided never had, hands the caller
/// to the checkout fallback, whose reason is the bootstrap's answer when it
/// fails too. Returns the pane reason alongside a checkout-bound attestation.
fn attest_bootstrap(
    peer: i32,
    request: &BootstrapRequest,
    herdr_socket: Option<&Path>,
    core: &CoreHandle,
) -> Result<(Attestation, Option<&'static str>), &'static str> {
    // The encoded checkout caller is hided's own key for the core; a request
    // never names one, whatever Herdr's pane namespace holds in the future.
    if matches!(Caller::parse(&request.pane_id), Caller::Checkout { .. }) {
        return Err("invalid_request");
    }
    let pane = if request.pane_id.is_empty() {
        Err("pane_not_connected")
    } else {
        match herdr_socket {
            Some(socket) => attest_local(peer, &request.pane_id, socket, core),
            None => Err("pane_unavailable"),
        }
    };
    match pane {
        Ok(attestation) => Ok((attestation, None)),
        Err(pane_reason) => attest_checkout(peer, &request.nonce, core)
            .map(|attestation| (attestation, Some(pane_reason)))
            .inspect_err(|reason| {
                note_checkout_capability(
                    "checkout_capability.refused",
                    "bootstrap",
                    Some(peer),
                    Some(!request.one_shot),
                    None,
                    Some(reason),
                    Some(pane_reason),
                );
            }),
    }
}

/// Binds a local caller that is not a pane descendant to the registered,
/// connected checkout holding its working directory. The directory comes
/// from the kernel for the peer pid; the request supplies nothing but the
/// nonce, which becomes the capability's key in the core's caller id.
pub fn attest_checkout(
    peer: i32,
    nonce: &str,
    core: &CoreHandle,
) -> Result<Attestation, &'static str> {
    if nonce.len() != 32 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("invalid_nonce");
    }
    let cwd = process_cwd(peer).ok_or("caller_unavailable")?;
    let canonical = fs::canonicalize(cwd).map_err(|_| "caller_unavailable")?;
    let path = canonical.to_str().ok_or("caller_unavailable")?;
    let caller_id = checkout_caller_id(nonce, path);
    let context = core
        .workspace_query("local", &caller_id, Query::Info)
        .map_err(|refusal| refusal.reason)?
        .context;
    Ok(Attestation {
        pane_id: caller_id,
        context,
        binding: Binding::Checkout,
    })
}

/// One record per checkout-bound issuance or refusal, in the core's Logs
/// file. It names the device, checkout and peer pid, never the caller's
/// path, pane id, or reference bytes.
fn note_checkout_capability(
    kind: &str,
    stage: &str,
    peer: Option<i32>,
    persistent: Option<bool>,
    context: Option<&Context>,
    reason: Option<&str>,
    pane_reason: Option<&str>,
) {
    herdr_core::diagnostic!(json!({
        "component": "pane_auth",
        "kind": kind,
        "stage": stage,
        "peer_pid": peer,
        "persistent": persistent,
        "device_id": context.map(|context| context.device_id.as_str()),
        "checkout_id": context.map(|context| context.checkout_id.as_str()),
        "reason": reason,
        "pane_reason": pane_reason,
    }));
}

pub fn attest_remote(
    core: &CoreHandle,
    device_id: &str,
    source_pane_id: &str,
    identity: &hide_host::workspace_bridge::PaneIdentity,
) -> Result<Attestation, &'static str> {
    if device_id.is_empty() || source_pane_id.is_empty() || source_pane_id.len() > 256 {
        return Err("invalid_request");
    }
    let pane_id = format!("remote:{device_id}:pane:{source_pane_id}");
    let context = core
        .workspace_query(device_id, &pane_id, Query::Info)
        .map_err(|_| "pane_not_connected")?
        .context;
    Ok(Attestation {
        pane_id,
        context,
        binding: Binding::Pane {
            terminal_id: identity.terminal_id.clone(),
            shell_pid: identity.shell_pid,
            shell_started: identity.shell_started,
        },
    })
}

pub fn attest_local(
    peer: i32,
    pane_id: &str,
    herdr_socket: &Path,
    core: &CoreHandle,
) -> Result<Attestation, &'static str> {
    let attestation = inspect_pane(pane_id, herdr_socket, core)?;
    let Binding::Pane { shell_pid, .. } = &attestation.binding else {
        return Err("pane_unavailable");
    };
    if !descends_from(peer, *shell_pid) {
        return Err("caller_not_in_pane");
    }
    Ok(attestation)
}

fn inspect_pane(
    pane_id: &str,
    herdr_socket: &Path,
    core: &CoreHandle,
) -> Result<Attestation, &'static str> {
    let connector = UnixSocketConnector::new(herdr_socket);
    let value = request_with_connector(
        &connector,
        "pane.process_info",
        json!({"pane_id": pane_id}),
        HERDR_TIMEOUT,
    )
    .map_err(|_| "pane_unavailable")?;
    let shell = value
        .pointer("/process_info/shell_pid")
        .and_then(serde_json::Value::as_u64)
        .ok_or("pane_unavailable")?;
    if shell > i32::MAX as u64 {
        return Err("pane_unavailable");
    }
    let shell_pid = shell as i32;
    let shell_started = process_start(shell_pid).ok_or("pane_unavailable")?;
    let pane = request_with_connector(
        &connector,
        "pane.get",
        json!({"pane_id": pane_id}),
        HERDR_TIMEOUT,
    )
    .map_err(|_| "pane_unavailable")?;
    let terminal_id = pane
        .pointer("/pane/terminal_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("pane_unavailable")?
        .to_owned();
    let context = core
        .workspace_query("local", pane_id, Query::Info)
        .map_err(|_| "pane_not_connected")?;
    Ok(Attestation {
        pane_id: pane_id.to_owned(),
        context: context.context,
        binding: Binding::Pane {
            terminal_id,
            shell_pid,
            shell_started,
        },
    })
}

#[cfg(target_os = "macos")]
pub fn peer_pid(stream: &impl AsRawFd) -> Option<i32> {
    let mut pid: libc::pid_t = 0;
    let mut size = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: both output pointers refer to initialized stack values of the
    // declared sizes, and the fd remains owned by `stream` for this call.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut size,
        )
    };
    (result == 0 && size as usize == std::mem::size_of::<libc::pid_t>() && pid > 0).then_some(pid)
}

#[cfg(target_os = "linux")]
pub fn peer_pid(stream: &impl AsRawFd) -> Option<i32> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `credentials` and `size` are writable for their declared sizes.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut size,
        )
    };
    (result == 0 && size as usize == std::mem::size_of::<libc::ucred>() && credentials.pid > 0)
        .then_some(credentials.pid)
}

pub fn descends_from(mut pid: i32, shell_pid: i32) -> bool {
    for _ in 0..MAX_PARENT_HOPS {
        if pid == shell_pid {
            return true;
        }
        if pid <= 1 {
            return false;
        }
        let Some(parent) = parent_pid(pid) else {
            return false;
        };
        if parent == pid {
            return false;
        }
        pid = parent;
    }
    false
}

#[cfg(target_os = "macos")]
fn parent_pid(pid: i32) -> Option<i32> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    // SAFETY: the buffer is valid for its declared size and only read after
    // `proc_pidinfo` confirms it filled the complete structure.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            std::mem::size_of::<libc::proc_bsdinfo>() as i32,
        )
    };
    (written as usize == std::mem::size_of::<libc::proc_bsdinfo>()).then_some(info.pbi_ppid as i32)
}

/// The peer's current directory as the kernel reports it, or `None` when the
/// process is gone or refuses inspection.
#[cfg(target_os = "macos")]
fn process_cwd(pid: i32) -> Option<PathBuf> {
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>();
    // SAFETY: the buffer is valid for its declared size and only read after
    // `proc_pidinfo` confirms it filled the complete structure.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&mut info as *mut libc::proc_vnodepathinfo).cast(),
            size as i32,
        )
    };
    if written as usize != size {
        return None;
    }
    // libc declares the MAXPATHLEN buffer as 32 rows of 32 for old compilers;
    // the path is the NUL-terminated prefix of the flattened bytes.
    let bytes: Vec<u8> = info
        .pvi_cdir
        .vip_path
        .iter()
        .flatten()
        .map(|&byte| byte as u8)
        .take_while(|&byte| byte != 0)
        .collect();
    (!bytes.is_empty()).then(|| PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

#[cfg(target_os = "linux")]
fn process_cwd(pid: i32) -> Option<PathBuf> {
    fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(target_os = "macos")]
pub(crate) fn process_start(pid: i32) -> Option<u64> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    // SAFETY: the buffer remains valid and is read only after a full result.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            std::mem::size_of::<libc::proc_bsdinfo>() as i32,
        )
    };
    (written as usize == std::mem::size_of::<libc::proc_bsdinfo>())
        .then_some((info.pbi_start_tvsec as u64) * 1_000_000 + info.pbi_start_tvusec as u64)
}

pub fn bind(state_dir: &Path) -> Result<(UnixListener, PathBuf), String> {
    // A random private leaf keeps the Unix path short even for a long state
    // directory, and cannot be pre-created by another /tmp user.
    let directory = (0..8)
        .find_map(|_| {
            let candidate = Path::new("/tmp").join(format!("hide-pane-{}", &new_token()[..24]));
            match fs::DirBuilder::new().mode(0o700).create(&candidate) {
                Ok(()) => Some(Ok(candidate)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(error.to_string())),
            }
        })
        .unwrap_or_else(|| Err("pane bootstrap directory collision limit".to_owned()))?;
    let path = directory.join("b.sock");
    let result = (|| {
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
        let record = bootstrap_socket_record(state_dir);
        let staging = record.with_extension(format!("{}.tmp", &new_token()[..16]));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staging)
            .map_err(|error| error.to_string())?;
        let published = (|| {
            file.write_all(path.as_os_str().as_bytes())
                .map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            fs::rename(&staging, &record).map_err(|error| error.to_string())
        })();
        if published.is_err() {
            let _ = fs::remove_file(staging);
        }
        published?;
        Ok((listener, path.clone()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&directory);
    }
    result
}

pub fn bootstrap_socket_record(state_dir: &Path) -> PathBuf {
    state_dir.join("pane-capabilities/bootstrap-socket")
}

pub fn bootstrap_socket_path(state_dir: &Path) -> Result<PathBuf, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(bootstrap_socket_record(state_dir))
        .map_err(|_| "hide_unavailable".to_owned())?;
    let metadata = file.metadata().map_err(|_| "hide_unavailable".to_owned())?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > 100
    {
        return Err("invalid_bootstrap_socket_record".to_owned());
    }
    let mut bytes = Vec::new();
    file.take(100)
        .read_to_end(&mut bytes)
        .map_err(|_| "invalid_bootstrap_socket_record".to_owned())?;
    let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    let directory = path.parent().ok_or("invalid_bootstrap_socket_record")?;
    let metadata = fs::symlink_metadata(directory)
        .map_err(|_| "invalid_bootstrap_socket_record".to_owned())?;
    if !path.is_absolute()
        || !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err("invalid_bootstrap_socket_record".to_owned());
    }
    Ok(path)
}

pub async fn serve(
    listener: UnixListener,
    registry: Arc<Registry>,
    core: Arc<CoreHandle>,
    herdr_socket: Option<PathBuf>,
    port: u16,
    shutdown: Arc<Notify>,
) {
    let limit = Arc::new(Semaphore::new(MAX_BOOTSTRAPS));
    let mut sweep = tokio::time::interval(Duration::from_secs(5));
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            _ = sweep.tick() => {
                let registry = Arc::clone(&registry);
                let core = Arc::clone(&core);
                tokio::task::spawn_blocking(move || registry.sweep(&core));
                continue;
            }
            _ = shutdown.notified() => break,
        };
        let Ok((stream, _)) = accepted else { break };
        let Ok(permit) = Arc::clone(&limit).try_acquire_owned() else {
            continue;
        };
        let registry = Arc::clone(&registry);
        let core = Arc::clone(&core);
        let herdr_socket = herdr_socket.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let Some(peer) = peer_pid(&stream) else {
                return;
            };
            let Ok(stream) = stream.into_std() else {
                return;
            };
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(BOOTSTRAP_TIMEOUT));
            let _ = stream.set_write_timeout(Some(BOOTSTRAP_TIMEOUT));
            let mut line = String::new();
            let result = BufReader::new(&stream)
                .take(4096)
                .read_line(&mut line)
                .map_err(|_| "invalid_request")
                .and_then(|_| {
                    serde_json::from_str::<BootstrapRequest>(&line).map_err(|_| "invalid_request")
                })
                .and_then(|request| {
                    let (attestation, pane_reason) =
                        attest_bootstrap(peer, &request, herdr_socket.as_deref(), &core)?;
                    let holder = if request.one_shot {
                        Some((peer, process_start(peer).ok_or("caller_unavailable")?))
                    } else {
                        None
                    };
                    let issued = registry.issue(&attestation, &request.nonce, port, holder);
                    if let Some(pane_reason) = pane_reason {
                        note_checkout_capability(
                            match issued {
                                Ok(_) => "checkout_capability.issued",
                                Err(_) => "checkout_capability.refused",
                            },
                            "bootstrap",
                            Some(peer),
                            Some(!request.one_shot),
                            Some(&attestation.context),
                            issued.as_ref().err().copied(),
                            Some(pane_reason),
                        );
                    }
                    issued
                });
            let answer = match &result {
                Ok((path, _)) => json!({"ok": true, "reference": path}),
                Err(reason) => json!({"ok": false, "reason": reason}),
            };
            let mut stream = stream;
            if writeln!(stream, "{answer}").is_err()
                && let Ok((path, true)) = result
                && let Ok(bytes) = fs::read(path)
                && let Ok(reference) = serde_json::from_slice::<Reference>(&bytes)
            {
                registry.revoke(&reference.token);
            }
        });
    }
}

#[cfg(target_os = "linux")]
fn parent_pid(pid: i32) -> Option<i32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[cfg(target_os = "linux")]
pub(crate) fn process_start(pid: i32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn long_state_directory_keeps_a_short_private_bootstrap_socket() {
        let directory = tempfile::tempdir().unwrap();
        let state_dir = directory.path().join("long-state-segment/".repeat(12));
        fs::create_dir_all(state_dir.join("pane-capabilities")).unwrap();
        let (listener, socket) = bind(&state_dir).unwrap();
        assert!(socket.as_os_str().as_bytes().len() < 100);
        assert_eq!(socket, bootstrap_socket_path(&state_dir).unwrap());
        assert_eq!(
            fs::metadata(socket.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        drop(listener);
        fs::remove_file(&socket).unwrap();
        fs::remove_dir(socket.parent().unwrap()).unwrap();
        let (listener, next) = bind(&state_dir).unwrap();
        assert_ne!(socket, next);
        assert_eq!(next, bootstrap_socket_path(&state_dir).unwrap());
        drop(listener);
        fs::remove_file(&next).unwrap();
        fs::remove_dir(next.parent().unwrap()).unwrap();
    }

    #[test]
    fn registry_releases_references_and_refuses_issuance_after_shutdown() {
        let directory = tempfile::tempdir().unwrap();
        let registry = Registry::new(directory.path()).unwrap();
        let pid = std::process::id() as i32;
        let attestation = Attestation {
            pane_id: "pane".to_owned(),
            context: Context {
                device_id: "local".to_owned(),
                workspace_id: "workspace".to_owned(),
                checkout_id: "checkout".to_owned(),
                checkout_path: "/checkout".to_owned(),
            },
            binding: Binding::Pane {
                terminal_id: "terminal".to_owned(),
                shell_pid: pid,
                shell_started: process_start(pid).unwrap(),
            },
        };
        let nonce = "0123456789abcdef0123456789abcdef";
        let path = registry.issue(&attestation, nonce, 12345, None).unwrap().0;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let reference: Reference = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(registry.get(&reference.token).is_some());
        registry.revoke_all();
        assert!(!path.exists());
        assert!(registry.get(&reference.token).is_none());
        assert_eq!(
            registry
                .issue(&attestation, nonce, 12345, None)
                .unwrap_err(),
            "hide_unavailable"
        );
    }

    #[test]
    fn bootstrap_retries_do_not_fill_the_registry_when_no_client_claims() {
        let directory = tempfile::tempdir().unwrap();
        let registry = Registry::new(directory.path()).unwrap();
        let core = bare_core(directory.path());
        let pid = std::process::id() as i32;
        let attestation = Attestation {
            pane_id: "pane".to_owned(),
            context: Context {
                device_id: "local".to_owned(),
                workspace_id: "workspace".to_owned(),
                checkout_id: "checkout".to_owned(),
                checkout_path: "/checkout".to_owned(),
            },
            binding: Binding::Pane {
                terminal_id: "terminal".to_owned(),
                shell_pid: pid,
                shell_started: process_start(pid).unwrap(),
            },
        };
        for attempt in 0..(MAX_CAPABILITIES * 2) {
            let nonce = format!("{attempt:032x}");
            let path = registry.issue(&attestation, &nonce, 12345, None).unwrap().0;
            let reference: Reference = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            registry
                .entries
                .lock()
                .unwrap()
                .get_mut(&reference.token)
                .unwrap()
                .created = Instant::now() - UNCLAIMED_LIFETIME;
            registry.sweep(&core);
            assert!(!path.exists());
        }
        assert!(registry.entries.lock().unwrap().is_empty());
        assert_eq!(
            fs::read_dir(directory.path().join("pane-capabilities"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn repeated_claimed_bootstrap_reuses_one_pane_capability() {
        let directory = tempfile::tempdir().unwrap();
        let registry = Registry::new(directory.path()).unwrap();
        let pid = std::process::id() as i32;
        let attestation = Attestation {
            pane_id: "pane".to_owned(),
            context: Context {
                device_id: "local".to_owned(),
                workspace_id: "workspace".to_owned(),
                checkout_id: "checkout".to_owned(),
                checkout_path: "/checkout".to_owned(),
            },
            binding: Binding::Pane {
                terminal_id: "terminal".to_owned(),
                shell_pid: pid,
                shell_started: process_start(pid).unwrap(),
            },
        };
        let (path, created) = registry
            .issue(&attestation, &format!("{:032x}", 0), 12345, None)
            .unwrap();
        assert!(created);
        let reference: Reference = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        registry.claim(&reference.token).unwrap();
        for attempt in 1..=MAX_CAPABILITIES + 1 {
            let (again, created) = registry
                .issue(&attestation, &format!("{attempt:032x}"), 12345, None)
                .unwrap();
            assert_eq!(again, path);
            assert!(!created);
        }
        let (one_shot, created) = registry
            .issue(
                &attestation,
                &format!("{:032x}", MAX_CAPABILITIES + 2),
                12345,
                Some((pid, process_start(pid).unwrap())),
            )
            .unwrap();
        assert!(created);
        assert_ne!(one_shot, path);
        assert_eq!(registry.entries.lock().unwrap().len(), 2);
    }

    /// A core with no Herdr and no registered checkout, so every bootstrap
    /// answer below comes from the attestation path itself.
    fn bare_core(directory: &Path) -> CoreHandle {
        CoreHandle::spawn(herdr_core::CoreOptions {
            schema_version: crate::state_file::SCHEMA_VERSION,
            machine_id: None,
            herdr_socket_path: None,
            herdr_bin_path: None,
            app_state_path: directory.join("core-state.json").display().to_string(),
            host_helper_dir: None,
            host_helper_root: None,
            host_cli_dir: None,
            workspace_views_path: None,
            shortcut_import_path: None,
            local_issues_path: None,
        })
        .unwrap()
    }

    #[test]
    fn bootstrap_without_a_herdr_socket_still_reaches_the_checkout_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let core = bare_core(directory.path());
        let peer = std::process::id() as i32;
        let request = |pane_id: &str| BootstrapRequest {
            pane_id: pane_id.to_owned(),
            nonce: format!("{:032x}", 7),
            one_shot: true,
        };
        // A stale pane id with no Herdr socket used to answer pane_unavailable
        // before the fallback ran; the fallback's own answer is the proof it ran
        // (this test's cwd is inside no registered checkout).
        assert_eq!(
            attest_bootstrap(peer, &request("w9J:p17"), None, &core).unwrap_err(),
            "checkout_not_registered"
        );
        assert_eq!(
            attest_bootstrap(peer, &request(""), None, &core).unwrap_err(),
            "checkout_not_registered"
        );
        // A request cannot name the core's encoded checkout caller itself.
        assert_eq!(
            attest_bootstrap(peer, &request("checkout:abcd:/tmp"), None, &core).unwrap_err(),
            "invalid_request"
        );
        // An unreadable caller fails closed before any lookup.
        assert_eq!(
            attest_checkout(i32::MAX, &format!("{:032x}", 8), &core).unwrap_err(),
            "caller_unavailable"
        );
        assert_eq!(
            attest_checkout(peer, "not-a-nonce", &core).unwrap_err(),
            "invalid_nonce"
        );
        core.shutdown();
    }

    #[test]
    fn process_cwd_reads_the_callers_working_directory() {
        let expected = std::env::current_dir().unwrap().canonicalize().unwrap();
        let cwd = process_cwd(std::process::id() as i32).unwrap();
        assert_eq!(cwd.canonicalize().unwrap(), expected);
        assert!(process_cwd(i32::MAX).is_none());
    }

    fn checkout_attestation(nonce: &str, checkout: &str) -> Attestation {
        Attestation {
            pane_id: checkout_caller_id(nonce, checkout),
            context: Context {
                device_id: "local".to_owned(),
                workspace_id: "workspace".to_owned(),
                checkout_id: format!("checkout{checkout}"),
                checkout_path: checkout.to_owned(),
            },
            binding: Binding::Checkout,
        }
    }

    #[test]
    fn sweep_drops_a_checkout_capability_whose_checkout_no_longer_resolves() {
        let directory = tempfile::tempdir().unwrap();
        let registry = Registry::new(directory.path()).unwrap();
        let core = bare_core(directory.path());
        let pid = std::process::id() as i32;
        let gone = checkout_attestation(&format!("{:032x}", 3), "/srv/gone");
        let (path, created) = registry
            .issue(&gone, &format!("{:032x}", 3), 12345, None)
            .unwrap();
        assert!(created);
        let reference: Reference = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        registry.claim(&reference.token).unwrap();
        let pane = Attestation {
            pane_id: "w1:p1".to_owned(),
            context: Context {
                device_id: "local".to_owned(),
                workspace_id: "workspace".to_owned(),
                checkout_id: "checkout".to_owned(),
                checkout_path: "/srv/pane".to_owned(),
            },
            binding: Binding::Pane {
                terminal_id: "terminal".to_owned(),
                shell_pid: pid,
                shell_started: process_start(pid).unwrap(),
            },
        };
        let (pane_path, _) = registry
            .issue(&pane, &format!("{:032x}", 4), 12345, None)
            .unwrap();
        assert_eq!(registry.entries.lock().unwrap().len(), 2);
        // The bare core registers no checkout, so the checkout-bound entry is
        // what an unregistered project looks like; the pane entry is not
        // re-resolved by the sweep and stays until its shell ends.
        registry.sweep(&core);
        assert_eq!(registry.entries.lock().unwrap().len(), 1);
        assert!(!path.exists());
        assert!(pane_path.exists());
        assert_eq!(
            registry.validate(&reference.token, None, &core).err(),
            Some("credential_expired")
        );
        core.shutdown();
    }

    #[test]
    fn checkout_bound_bootstraps_reuse_one_persistent_capability_across_nonces() {
        let directory = tempfile::tempdir().unwrap();
        let registry = Registry::new(directory.path()).unwrap();
        let first = format!("{:032x}", 1);
        let (path, created) = registry
            .issue(
                &checkout_attestation(&first, "/checkout"),
                &first,
                12345,
                None,
            )
            .unwrap();
        assert!(created);
        let reference: Reference = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        // Alive without any pane shell behind it.
        let capability = registry.get(&reference.token).unwrap();
        assert_eq!(capability.pane_id, checkout_caller_id(&first, "/checkout"));
        assert_eq!(capability.context.checkout_path, "/checkout");
        registry.claim(&reference.token).unwrap();

        let second = format!("{:032x}", 2);
        let (again, created) = registry
            .issue(
                &checkout_attestation(&second, "/checkout"),
                &second,
                12345,
                None,
            )
            .unwrap();
        assert_eq!(again, path);
        assert!(!created);

        let other = format!("{:032x}", 3);
        let (elsewhere, created) = registry
            .issue(&checkout_attestation(&other, "/other"), &other, 12345, None)
            .unwrap();
        assert!(created);
        assert_ne!(elsewhere, path);

        let pid = std::process::id() as i32;
        let fourth = format!("{:032x}", 4);
        let (one_shot, created) = registry
            .issue(
                &checkout_attestation(&fourth, "/checkout"),
                &fourth,
                12345,
                Some((pid, process_start(pid).unwrap())),
            )
            .unwrap();
        assert!(created);
        assert_ne!(one_shot, path);
        assert_eq!(registry.entries.lock().unwrap().len(), 3);
    }
}
