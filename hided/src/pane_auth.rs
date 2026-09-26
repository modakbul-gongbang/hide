//! Bootstrap of short-lived pane credentials. This local socket does only
//! kernel peer attestation and credential issuance; commands use `/ws`.

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
use herdr_core::workspace_control::{Context, Query};
use hide_herdr_client::{UnixSocketConnector, request_with_connector};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::UnixListener;
use tokio::sync::{Notify, Semaphore};

use crate::core::CoreHandle;
use crate::state_file::new_token;

const MAX_CAPABILITIES: usize = 64;
const CAPABILITY_LIFETIME: Duration = Duration::from_secs(8 * 60 * 60);
const UNCLAIMED_LIFETIME: Duration = Duration::from_secs(30);
const HERDR_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PARENT_HOPS: usize = 32;
const MAX_BOOTSTRAPS: usize = 8;
const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct Capability {
    pub pane_id: String,
    pub context: Context,
    terminal_id: String,
    shell_pid: i32,
    shell_started: u64,
    pub one_shot: bool,
    holder: Option<(i32, u64)>,
    created: Instant,
    claimed: bool,
    path: PathBuf,
    remote: Option<RemoteCapability>,
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
    entry.path.is_file()
        && process_start(entry.shell_pid) == Some(entry.shell_started)
        && entry
            .holder
            .is_none_or(|(pid, born)| process_start(pid) == Some(born))
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
                !entry.one_shot
                    && entry.remote.is_none()
                    && entry.pane_id == attestation.pane_id
                    && entry.context == attestation.context
                    && entry.terminal_id == attestation.terminal_id
                    && entry.shell_pid == attestation.shell_pid
                    && entry.shell_started == attestation.shell_started
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
                terminal_id: attestation.terminal_id.clone(),
                shell_pid: attestation.shell_pid,
                shell_started: attestation.shell_started,
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
                    && entry.pane_id == attestation.pane_id
                    && entry.context == attestation.context
                    && entry.terminal_id == attestation.terminal_id
                    && entry.shell_pid == attestation.shell_pid
                    && entry.shell_started == attestation.shell_started
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
                terminal_id: attestation.terminal_id.clone(),
                shell_pid: attestation.shell_pid,
                shell_started: attestation.shell_started,
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
                terminal_id: identity.terminal_id,
                shell_pid: identity.shell_pid,
                shell_started: identity.shell_started,
            }
        } else {
            let socket = herdr_socket.ok_or("pane_unavailable")?;
            inspect_pane(&cap.pane_id, socket, core).inspect_err(|_| self.revoke(token))?
        };
        if actual.context != cap.context
            || actual.terminal_id != cap.terminal_id
            || actual.shell_pid != cap.shell_pid
            || actual.shell_started != cap.shell_started
        {
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

    pub fn sweep(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|_, entry| {
                let alive = entry_alive(entry);
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
/// to be a descendant of the shell Herdr owns for this pane.
#[derive(Deserialize)]
pub struct BootstrapRequest {
    pub pane_id: String,
    pub nonce: String,
    #[serde(default)]
    pub one_shot: bool,
}

#[derive(Clone, Debug)]
pub struct Attestation {
    pane_id: String,
    context: Context,
    terminal_id: String,
    shell_pid: i32,
    shell_started: u64,
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
        terminal_id: identity.terminal_id.clone(),
        shell_pid: identity.shell_pid,
        shell_started: identity.shell_started,
    })
}

pub fn attest_local(
    peer: i32,
    pane_id: &str,
    herdr_socket: &Path,
    core: &CoreHandle,
) -> Result<Attestation, &'static str> {
    let attestation = inspect_pane(pane_id, herdr_socket, core)?;
    if !descends_from(peer, attestation.shell_pid) {
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
        terminal_id,
        shell_pid,
        shell_started,
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
                registry.sweep();
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
                    let socket = herdr_socket.as_deref().ok_or("pane_unavailable")?;
                    let attestation = attest_local(peer, &request.pane_id, socket, &core)?;
                    let holder = if request.one_shot {
                        Some((peer, process_start(peer).ok_or("caller_unavailable")?))
                    } else {
                        None
                    };
                    registry.issue(&attestation, &request.nonce, port, holder)
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
            terminal_id: "terminal".to_owned(),
            shell_pid: pid,
            shell_started: process_start(pid).unwrap(),
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
        let pid = std::process::id() as i32;
        let attestation = Attestation {
            pane_id: "pane".to_owned(),
            context: Context {
                device_id: "local".to_owned(),
                workspace_id: "workspace".to_owned(),
                checkout_id: "checkout".to_owned(),
                checkout_path: "/checkout".to_owned(),
            },
            terminal_id: "terminal".to_owned(),
            shell_pid: pid,
            shell_started: process_start(pid).unwrap(),
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
            registry.sweep();
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
            terminal_id: "terminal".to_owned(),
            shell_pid: pid,
            shell_started: process_start(pid).unwrap(),
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
}
