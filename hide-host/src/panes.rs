//! A device's panes on its node link (PRD core-host-node B18, B19). The node
//! listens on an owner-only local stream (`hide_platform::ipc`: a Unix socket,
//! a named pipe on Windows) in a fresh `bridge-*` folder of the account's
//! `workspace-bridges` folder, where a pane's `hide` finds it; several cores
//! may serve one device, so each node keeps its own folder.
//!
//! A caller asks either for a credential or for a stream. A credential
//! request is proved with this machine's kernel (the pid the system reports
//! for the stream's other end, descended from the pane's shell) and sent up
//! the link as [`NodeEvent::PaneProof`]; the core answers down, and the node
//! writes the reference file the caller reads. A stream carries one `hide`
//! command's bytes up and down the same link. Everything here lives only as
//! long as the link: the node's input closing ends the listener, every
//! stream and every reference with it.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock, mpsc};
use std::thread::Scope;
use std::time::{Duration, Instant};

use base64::Engine as _;
use hide_node_link::panes::{
    MAX_CHUNK, MAX_STREAMS, NodeEvent, PaneIdentity, PanesStarted, ProofAnswer,
};
use hide_platform::fs::private;
use hide_platform::ipc::{ListenerCloser, LocalListener, LocalStream, ShutdownHandle};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{ErrorCode, HostError, HostResult};
use crate::pane_peer;

const CLIENT_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_CONTROL_LINE: u64 = 4096;
const UNCLAIMED_LIFETIME: Duration = Duration::from_secs(30);
const REFERENCE_LIFETIME: Duration = Duration::from_secs(8 * 60 * 60);
const SWEEP_INTERVAL: Duration = Duration::from_secs(5);
/// Callers served at once; more wait in the system's backlog.
const MAX_CLIENTS: usize = 16;
/// How old a `bridge-*` folder whose socket does not answer must be before
/// a starting service removes it.
const DEAD_FOLDER_AGE: Duration = Duration::from_secs(60);
/// How long a write to a stream's caller may block before the stream ends.
const STREAM_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

struct IssuedReference {
    token: String,
    created: Instant,
    holder: Option<(i32, u64)>,
    shell_pid: i32,
    shell_started: u64,
}

/// A credential request, as a pane's `hide` sends it.
#[derive(Deserialize)]
struct Bootstrap {
    pane_id: String,
    nonce: String,
    #[serde(default)]
    one_shot: bool,
}

struct Started {
    herdr_socket: PathBuf,
    socket_wire: String,
    folder: PathBuf,
    closer: ListenerCloser,
}

/// The pane service of one node link. Created idle with the serve loop and
/// started by the core's `panes_start`.
pub struct Panes {
    started: OnceLock<Started>,
    start_lock: Mutex<()>,
    stopped: AtomicBool,
    issued: Mutex<HashMap<PathBuf, IssuedReference>>,
    proofs: Mutex<HashMap<u64, mpsc::SyncSender<ProofAnswer>>>,
    streams: Mutex<HashMap<u64, StreamEnd>>,
    clients: AtomicUsize,
    next: AtomicU64,
}

struct StreamEnd {
    writer: LocalStream,
    shutdown: ShutdownHandle,
}

impl Default for Panes {
    fn default() -> Self {
        Self::new()
    }
}

impl Panes {
    pub fn new() -> Self {
        Self {
            started: OnceLock::new(),
            start_lock: Mutex::new(()),
            stopped: AtomicBool::new(false),
            issued: Mutex::new(HashMap::new()),
            proofs: Mutex::new(HashMap::new()),
            streams: Mutex::new(HashMap::new()),
            clients: AtomicUsize::new(0),
            next: AtomicU64::new(1),
        }
    }

    /// Binds the listener in a new folder under `bridges` and starts serving
    /// it on `scope`. Asked again for the same Herdr, answers the same
    /// socket; another Herdr is refused, since a node serves one.
    pub fn start<'scope, 'env, W: Write + Send>(
        &'scope self,
        scope: &'scope Scope<'scope, 'env>,
        output: &'scope Mutex<W>,
        bridges: &Path,
        herdr_socket: &str,
    ) -> HostResult<PanesStarted> {
        let herdr_socket = PathBuf::from(herdr_socket);
        if !herdr_socket.is_absolute() {
            return Err(HostError::new(
                ErrorCode::InvalidPath,
                "The Herdr socket is not an absolute path",
            ));
        }
        let _starting = self
            .start_lock
            .lock()
            .map_err(|_| HostError::new(ErrorCode::Io, "The pane service lock was poisoned"))?;
        if let Some(started) = self.started.get() {
            return if started.herdr_socket == herdr_socket {
                Ok(PanesStarted {
                    socket: started.socket_wire.clone(),
                })
            } else {
                Err(HostError::new(
                    ErrorCode::Conflict,
                    "This node already serves the panes of another Herdr server",
                ))
            };
        }
        if self.stopped.load(Ordering::Acquire) {
            return Err(HostError::new(ErrorCode::Io, "The node is stopping"));
        }
        let io = |error: io::Error| HostError::new(ErrorCode::Io, error.to_string());
        validate_dir(bridges).map_err(io)?;
        remove_dead_folders(bridges);
        let folder = tempfile::Builder::new()
            .prefix("bridge-")
            .tempdir_in(bridges)
            .map_err(io)?
            .keep();
        let socket = folder.join("bootstrap.sock");
        let bound = (|| {
            private::restrict_to_owner(&folder)?;
            let wire = hide_platform::path::to_wire(&socket)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
            let listener = LocalListener::bind(&socket)?;
            Ok::<_, io::Error>((wire, listener))
        })();
        let (socket_wire, listener) = match bound {
            Ok(bound) => bound,
            Err(error) => {
                let _ = fs::remove_dir_all(&folder);
                return Err(io(error));
            }
        };
        let started = Started {
            herdr_socket,
            socket_wire: socket_wire.clone(),
            folder,
            closer: listener.closer(),
        };
        if self.started.set(started).is_err() {
            unreachable!("only the start lock's holder sets the service");
        }
        scope.spawn(move || self.accept_loop(scope, output, listener));
        Ok(PanesStarted {
            socket: socket_wire,
        })
    }

    /// Ends the listener, every stream and every waiting proof, and removes
    /// the folder with the references in it. The serve loop calls it when
    /// its input ends.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        // Waits out a start in progress, so its listener is closed too.
        drop(self.start_lock.lock());
        if let Some(started) = self.started.get() {
            started.closer.close();
        }
        for (_, end) in lock(&self.streams).drain() {
            end.shutdown.shutdown();
        }
        lock(&self.proofs).clear();
    }

    /// Removes the service's folder; called once every thread of it ended.
    pub fn remove_folder(&self) {
        if let Some(started) = self.started.get() {
            let _ = fs::remove_dir_all(&started.folder);
        }
    }

    /// Hands the core's answer to the request waiting for it; one nobody
    /// waits for any more (its caller left, or it timed out) is dropped.
    pub fn answer_proof(&self, request: u64, answer: ProofAnswer) -> HostResult<Value> {
        if let Some(waiting) = lock(&self.proofs).remove(&request) {
            let _ = waiting.try_send(answer);
        }
        Ok(Value::Null)
    }

    /// `pane_id`'s identity now, from the Herdr this service was started for.
    pub fn inspect(&self, pane_id: &str) -> HostResult<PaneIdentity> {
        let started = self.started.get().ok_or_else(|| {
            HostError::new(ErrorCode::Unsupported, "The pane service is not started")
        })?;
        pane_peer::inspect(&started.herdr_socket, pane_id)
            .map_err(|reason| HostError::new(ErrorCode::NotFound, reason))
    }

    /// Writes base64 `data` to `stream`'s caller. A caller that does not
    /// take it in time loses its stream.
    pub fn write_stream(&self, stream: u64, data: &str) -> HostResult<Value> {
        if data.len() > MAX_CHUNK.div_ceil(3) * 4 {
            return Err(HostError::new(
                ErrorCode::TooLarge,
                "A stream chunk is larger than the link carries",
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| {
                HostError::new(ErrorCode::InvalidRequest, "A stream chunk is not base64")
            })?;
        let mut writer = {
            let streams = lock(&self.streams);
            let end = streams
                .get(&stream)
                .ok_or_else(|| HostError::new(ErrorCode::NotFound, "The stream has ended"))?;
            end.writer.duplicate()
        };
        if let Err(error) = writer.write_all(&bytes) {
            self.end_stream(stream);
            return Err(HostError::new(ErrorCode::Io, error.to_string()));
        }
        Ok(Value::Null)
    }

    /// Ends `stream` from the core's side; its reader then reports it closed.
    pub fn close_stream(&self, stream: u64) -> HostResult<Value> {
        self.end_stream(stream);
        Ok(Value::Null)
    }

    fn end_stream(&self, stream: u64) {
        if let Some(end) = lock(&self.streams).remove(&stream) {
            end.shutdown.shutdown();
        }
    }

    fn accept_loop<'scope, 'env, W: Write + Send>(
        &'scope self,
        scope: &'scope Scope<'scope, 'env>,
        output: &'scope Mutex<W>,
        listener: LocalListener,
    ) {
        // Bounded like the pane bootstrap's queue: a flood waits in the
        // system's backlog, not as open streams here.
        let (accepted, arrivals) = mpsc::sync_channel(MAX_CLIENTS);
        scope.spawn(move || {
            loop {
                let arrival = listener.accept();
                let failed = arrival.is_err();
                if accepted.send(arrival).is_err() || failed {
                    return;
                }
            }
        });
        let mut last_sweep = Instant::now();
        loop {
            if last_sweep.elapsed() >= SWEEP_INTERVAL {
                if self.sweep_references(output).is_err() {
                    return;
                }
                last_sweep = Instant::now();
            }
            let wait = SWEEP_INTERVAL.saturating_sub(last_sweep.elapsed());
            match arrivals.recv_timeout(wait) {
                Ok(Ok(mut stream)) => {
                    if self.clients.fetch_add(1, Ordering::AcqRel) >= MAX_CLIENTS {
                        self.clients.fetch_sub(1, Ordering::AcqRel);
                        let _ = answer_client(&mut stream, Err("bridge_busy".to_owned()));
                        report_refusal(output, None, "bridge_busy");
                        continue;
                    }
                    scope.spawn(move || {
                        self.serve_client(stream, output);
                        self.clients.fetch_sub(1, Ordering::AcqRel);
                    });
                }
                Ok(Err(_)) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            if self.stopped.load(Ordering::Acquire) {
                return;
            }
        }
    }

    fn serve_client(&self, mut stream: LocalStream, output: &Mutex<impl Write>) {
        let first = (|| {
            stream
                .set_read_timeout(Some(CLIENT_TIMEOUT))
                .map_err(|_| "bridge_unavailable")?;
            // A Windows pipe has no write timeout; the answer is one short line.
            let _ = stream.set_write_timeout(Some(CLIENT_TIMEOUT));
            // A byte at a time: a stream's first bytes follow its line at
            // once, and a buffered reader would keep them from the stream.
            let mut line = Vec::new();
            let mut byte = [0_u8; 1];
            while line.last() != Some(&b'\n') {
                if line.len() as u64 >= MAX_CONTROL_LINE
                    || stream.read(&mut byte).map_err(|_| "invalid_request")? == 0
                {
                    return Err("invalid_request");
                }
                line.push(byte[0]);
            }
            serde_json::from_slice::<Value>(&line).map_err(|_| "invalid_request")
        })();
        match first {
            Ok(request) if request.get("stream") == Some(&Value::Bool(true)) => {
                self.serve_stream(stream, output)
            }
            Ok(request) => self.serve_bootstrap(stream, request, output),
            Err(reason) => {
                let _ = answer_client(&mut stream, Err(reason.to_owned()));
                report_refusal(output, None, reason);
            }
        }
    }

    fn serve_bootstrap(&self, mut stream: LocalStream, request: Value, output: &Mutex<impl Write>) {
        // The pane the caller names, as far as a record should carry it.
        let named = request
            .get("pane_id")
            .and_then(Value::as_str)
            .filter(|pane_id| pane_id.len() <= 256)
            .map(str::to_owned);
        let Some(started) = self.started.get() else {
            let _ = answer_client(&mut stream, Err("bridge_unavailable".to_owned()));
            report_refusal(output, named, "bridge_unavailable");
            return;
        };
        let mut core_refused = false;
        let mut issued_token = None;
        let mut token_new = false;
        let mut reference_new = false;
        let result: Result<PathBuf, String> = (|| {
            let peer = stream
                .peer_pid()
                .ok()
                .and_then(|pid| i32::try_from(pid).ok())
                .ok_or("caller_unavailable")?;
            let request: Bootstrap =
                serde_json::from_value(request).map_err(|_| "invalid_request")?;
            if request.nonce.len() != 32
                || !request.nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err("invalid_nonce".to_owned());
            }
            let identity = pane_peer::inspect(&started.herdr_socket, &request.pane_id)?;
            if !pane_peer::descends_from(peer, identity.shell_pid) {
                return Err("caller_not_in_pane".to_owned());
            }
            let holder = if request.one_shot {
                Some((
                    peer,
                    pane_peer::process_start(peer).ok_or("caller_unavailable")?,
                ))
            } else {
                None
            };
            self.sweep_references(output)
                .map_err(|_| "bridge_unavailable")?;
            let id = self.next.fetch_add(1, Ordering::Relaxed);
            let (waiting, answer) = mpsc::sync_channel(1);
            lock(&self.proofs).insert(id, waiting);
            let sent = write_event(
                output,
                &NodeEvent::PaneProof {
                    request: id,
                    pane_id: request.pane_id.clone(),
                    identity: identity.clone(),
                    nonce: request.nonce.clone(),
                    one_shot: request.one_shot,
                },
            );
            let answer = sent
                .ok()
                .and_then(|()| answer.recv_timeout(CLIENT_TIMEOUT).ok());
            lock(&self.proofs).remove(&id);
            let token = match answer.ok_or("bridge_unavailable")? {
                ProofAnswer::Refused { reason } => {
                    core_refused = true;
                    return Err(reason);
                }
                ProofAnswer::Issued { token, issued_new } => {
                    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        return Err("invalid_response".to_owned());
                    }
                    token_new = issued_new;
                    token
                }
            };
            issued_token = Some(token.clone());
            let mut issued = lock(&self.issued);
            if !request.one_shot
                && let Some((path, _)) = issued.iter().find(|(path, reference)| {
                    reference.token == token && path.is_file() && reference.holder.is_none()
                })
            {
                return Ok(path.clone());
            }
            let path = started.folder.join(format!("{}.json", request.nonce));
            let mut file = private::create_new_file(&path).map_err(|_| "reference_unavailable")?;
            if write!(
                file,
                "{}",
                json!({"token": token, "socket": started.socket_wire})
            )
            .is_err()
                || file.sync_all().is_err()
            {
                let _ = fs::remove_file(&path);
                return Err("reference_unavailable".to_owned());
            }
            issued.insert(
                path.clone(),
                IssuedReference {
                    token,
                    created: Instant::now(),
                    holder,
                    shell_pid: identity.shell_pid,
                    shell_started: identity.shell_started,
                },
            );
            reference_new = true;
            Ok(path)
        })();
        let delivered = answer_client(&mut stream, result.clone()).is_ok();
        // The core records the refusals it answered itself.
        if let Err(reason) = &result
            && !core_refused
        {
            report_refusal(output, named, reason);
        }
        if !delivered || result.is_err() {
            if reference_new && let Ok(path) = result {
                let _ = fs::remove_file(&path);
                lock(&self.issued).remove(&path);
            }
            if token_new && let Some(token) = issued_token {
                let _ = write_event(output, &NodeEvent::Revoke { token });
            }
        }
    }

    fn serve_stream(&self, stream: LocalStream, output: &Mutex<impl Write>) {
        if stream.set_read_timeout(None).is_err() {
            return;
        }
        // The caller's writes are paced by the core; a caller that stops
        // reading loses its stream rather than holding a node worker.
        let _ = stream.set_write_timeout(Some(STREAM_WRITE_TIMEOUT));
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        {
            let mut streams = lock(&self.streams);
            if self.stopped.load(Ordering::Acquire) {
                return;
            }
            if streams.len() >= MAX_STREAMS {
                drop(streams);
                report_refusal(output, None, "streams_full");
                return;
            }
            streams.insert(
                id,
                StreamEnd {
                    writer: stream.duplicate(),
                    shutdown: stream.shutdown_handle(),
                },
            );
        }
        let mut reader = stream;
        if write_event(output, &NodeEvent::StreamOpen { stream: id }).is_ok() {
            let mut buffer = vec![0_u8; MAX_CHUNK];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        let data =
                            base64::engine::general_purpose::STANDARD.encode(&buffer[..read]);
                        if write_event(output, &NodeEvent::StreamData { stream: id, data }).is_err()
                        {
                            break;
                        }
                    }
                }
            }
        }
        self.end_stream(id);
        let _ = write_event(output, &NodeEvent::StreamClosed { stream: id });
    }

    fn sweep_references(&self, output: &Mutex<impl Write>) -> io::Result<()> {
        let mut revoked = Vec::new();
        {
            let mut issued = lock(&self.issued);
            let stale = issued
                .iter()
                .filter_map(|(path, reference)| {
                    let present = path.is_file();
                    let expired = reference.created.elapsed() >= REFERENCE_LIFETIME
                        || (!path.with_extension("claimed").is_file()
                            && reference.created.elapsed() >= UNCLAIMED_LIFETIME);
                    let holder_gone = reference.holder.is_some_and(|(pid, started)| {
                        pane_peer::process_start(pid) != Some(started)
                    });
                    let pane_gone = pane_peer::process_start(reference.shell_pid)
                        != Some(reference.shell_started);
                    (!present || expired || holder_gone || pane_gone).then_some(path.clone())
                })
                .collect::<Vec<_>>();
            for path in stale {
                remove_if_present(&path)?;
                remove_if_present(&path.with_extension("claimed"))?;
                if let Some(reference) = issued.remove(&path) {
                    revoked.push(reference.token);
                }
            }
        }
        for token in revoked {
            write_event(output, &NodeEvent::Revoke { token })?;
        }
        Ok(())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The folder a node's bootstrap folders go in: this account's own, never a
/// link, made private when the node creates it.
fn validate_dir(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bridge directory is not absolute",
        ));
    }
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (!metadata.is_dir() || metadata.file_type().is_symlink())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bridge directory is unsafe",
        ));
    }
    fs::create_dir_all(path)?;
    private::restrict_to_owner(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || !private::owned_by_current_user(path)?
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "bridge directory is not owned by this account",
        ));
    }
    Ok(())
}

/// Removes the folders of pane services that ended without removing their
/// own: a node that was killed, or the workspace bridge an older Hide ran.
/// A folder is dead when its socket no longer answers; one younger than a
/// minute may belong to a service still binding, so it is left.
fn remove_dead_folders(bridges: &Path) {
    let Ok(entries) = fs::read_dir(bridges) else {
        return;
    };
    for entry in entries.flatten() {
        let folder = entry.path();
        let young = fs::symlink_metadata(&folder)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_none_or(|age| age < DEAD_FOLDER_AGE);
        if !entry.file_name().to_string_lossy().starts_with("bridge-")
            || young
            || !entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        let socket = folder.join("bootstrap.sock");
        if fs::symlink_metadata(&socket).is_err() || LocalStream::connect(&socket).is_err() {
            let _ = fs::remove_dir_all(&folder);
        }
    }
}

/// One event line on the link's output, under the same lock as the answers,
/// so a line is never cut by another.
pub(crate) fn write_event(output: &Mutex<impl Write>, event: &NodeEvent) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(event).map_err(io::Error::other)?;
    bytes.push(b'\n');
    let mut output = output
        .lock()
        .map_err(|_| io::Error::other("node output lock poisoned"))?;
    output.write_all(&bytes)?;
    output.flush()
}

/// Tells the core this node turned a caller away, so the refusal is recorded
/// with the node it happened on.
fn report_refusal(output: &Mutex<impl Write>, pane_id: Option<String>, reason: &str) {
    let _ = write_event(
        output,
        &NodeEvent::Refused {
            pane_id,
            reason: reason.to_owned(),
        },
    );
}

/// A reference with no wire spelling is not answered at all, so its caller
/// reads the end of the stream and the issuer withdraws the reference.
fn answer_client(stream: &mut LocalStream, result: Result<PathBuf, String>) -> io::Result<()> {
    let answer = match result {
        Ok(reference) => {
            let reference = hide_platform::path::to_wire(&reference).map_err(io::Error::other)?;
            json!({"ok":true,"reference":reference})
        }
        Err(reason) => json!({"ok":false,"reason":reason}),
    };
    writeln!(stream, "{answer}")
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

// A folder's age is set through its handle, which only Unix opens.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A starting service removes what a killed node or an older Hide's
    /// bridge left, and never a folder whose socket answers or one another
    /// service may still be binding.
    #[test]
    fn a_start_removes_only_dead_folders() {
        let root = tempfile::Builder::new()
            .prefix("hp")
            .tempdir_in("/tmp")
            .unwrap();
        let bridges = root.path();
        let old = std::time::SystemTime::now() - 2 * DEAD_FOLDER_AGE;
        let folder = |name: &str| {
            let path = bridges.join(name);
            fs::create_dir(&path).unwrap();
            path
        };
        let age = |path: &Path| fs::File::open(path).unwrap().set_modified(old).unwrap();
        let dead = folder("bridge-dead");
        fs::write(dead.join("bootstrap.sock"), b"").unwrap();
        let empty = folder("bridge-empty");
        let live = folder("bridge-live");
        let _listener = LocalListener::bind(&live.join("bootstrap.sock")).unwrap();
        let young = folder("bridge-young");
        let other = folder("other");
        for path in [&dead, &empty, &live, &other] {
            age(path);
        }
        remove_dead_folders(bridges);
        assert!(!dead.exists());
        assert!(!empty.exists());
        assert!(live.exists());
        assert!(young.exists());
        assert!(other.exists());
    }
}
