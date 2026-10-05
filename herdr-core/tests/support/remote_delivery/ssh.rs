//! Real loopback SSH boundary, with fixture-only keys and owned channel jobs.
//! The account is a private HOME; streamlocal opens only its exact Herdr socket.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hide_platform::process::OwnedChild;
use russh::server::{self, ChannelOpenHandle, Msg, Session};
use russh::{Channel, ChannelId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UnixStream};
use tokio::sync::watch;
use tokio::task::JoinSet;

use super::Environment;

const CONNECTION_CAP: usize = 8;
const CHANNEL_CAP: usize = 32;
const JOB_CAP: usize = 128;
const STOP_BOUND: Duration = Duration::from_secs(5);

struct Shared {
    jobs: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    failures: Mutex<Vec<String>>,
    connections: Mutex<Vec<(server::Handle, watch::Sender<bool>)>>,
    environment: Environment,
    socket: PathBuf,
    client_key: russh::keys::PublicKey,
}

impl Shared {
    fn job(&self, work: impl Future<Output = ()> + Send + 'static) -> Result<()> {
        let mut jobs = self.jobs.lock().expect("SSH jobs");
        if jobs.len() >= JOB_CAP {
            bail!("private SSH channel job cap reached");
        }
        jobs.push(tokio::spawn(work));
        Ok(())
    }
}

enum Control {
    Online(bool, mpsc::SyncSender<()>),
    Stop,
}

pub struct Ssh {
    pub port: u16,
    controls: tokio::sync::mpsc::Sender<Control>,
    thread: Option<JoinHandle<()>>,
    finished: mpsc::Receiver<Result<()>>,
}

impl Ssh {
    pub fn start(
        environment: Environment,
        socket: PathBuf,
        host_key: &Path,
        client_key: &Path,
    ) -> Result<Self> {
        let host_key = russh::keys::load_secret_key(host_key, None)?;
        let client_key = russh::keys::load_secret_key(client_key, None)?
            .public_key()
            .clone();
        let config = Arc::new(server::Config {
            keys: vec![host_key],
            auth_rejection_time: Duration::from_millis(5),
            inactivity_timeout: Some(Duration::from_secs(60)),
            channel_buffer_size: 8,
            event_buffer_size: 32,
            ..server::Config::default()
        });
        let shared = Arc::new(Shared {
            jobs: Mutex::new(Vec::new()),
            failures: Mutex::new(Vec::new()),
            connections: Mutex::new(Vec::new()),
            environment,
            socket,
            client_key,
        });
        let (controls, receiver) = tokio::sync::mpsc::channel(4);
        let (ready, readiness) = mpsc::sync_channel(1);
        let (ended, finished) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("private-delivery-ssh".into())
            .spawn(move || {
                let result = (|| {
                    let runtime = tokio::runtime::Builder::new_multi_thread()
                        .worker_threads(2)
                        .enable_all()
                        .build()?;
                    runtime.block_on(serve(config, shared, receiver, ready))
                })();
                let _ = ended.send(result);
            })?;
        // Own the thread before readiness can fail, so a failed start also
        // reaches the same stop-and-join boundary.
        let mut ssh = Self {
            port: 0,
            controls,
            thread: Some(thread),
            finished,
        };
        ssh.port = readiness
            .recv_timeout(STOP_BOUND)
            .context("private SSH readiness")??;
        Ok(ssh)
    }

    pub fn online(&self, value: bool) -> Result<()> {
        let (ack, done) = mpsc::sync_channel(1);
        self.controls.blocking_send(Control::Online(value, ack))?;
        done.recv_timeout(STOP_BOUND)
            .context("private SSH disconnect barrier")?;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        if self.thread.is_none() {
            return Ok(());
        }
        self.controls.blocking_send(Control::Stop)?;
        self.finished
            .recv_timeout(STOP_BOUND)
            .context("private SSH cleanup unconfirmed")??;
        self.thread
            .take()
            .expect("SSH thread")
            .join()
            .map_err(|_| anyhow::anyhow!("private SSH thread panicked"))?;
        Ok(())
    }
}

impl Drop for Ssh {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("private SSH fixture cleanup failed: {error}");
        }
    }
}

async fn disconnect(shared: &Shared) {
    let connections = shared.connections.lock().expect("SSH connections").clone();
    for (handle, stop) in connections {
        let _ = stop.send(true);
        let _ = tokio::time::timeout(
            Duration::from_secs(1),
            handle.disconnect(
                russh::Disconnect::ByApplication,
                "fixture boundary closed".into(),
                "".into(),
            ),
        )
        .await;
    }
    shared.connections.lock().expect("SSH connections").clear();
}

async fn serve(
    config: Arc<server::Config>,
    shared: Arc<Shared>,
    mut controls: tokio::sync::mpsc::Receiver<Control>,
    ready: mpsc::SyncSender<Result<u16>>,
) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    ready.send(Ok(listener.local_addr()?.port()))?;
    let mut online = true;
    let mut sessions = JoinSet::new();
    loop {
        tokio::select! {
            control = controls.recv() => match control {
                Some(Control::Online(value, ack)) => {
                    online = value;
                    if !online { disconnect(&shared).await; }
                    let _ = ack.send(());
                }
                Some(Control::Stop) | None => break,
            },
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                // OpenSSH sets TCP_NODELAY on its sockets. Without it a reply
                // split over two writes waits for the client's delayed ACK
                // (about 40 ms on Linux), and the install's serial 32 KB SFTP
                // reads then take 46 ms each instead of 2.
                stream.set_nodelay(true)?;
                if !online || sessions.len() >= CONNECTION_CAP { continue; }
                let config = Arc::clone(&config);
                let shared = Arc::clone(&shared);
                sessions.spawn(async move {
                    let (stop, cancelled) = watch::channel(false);
                    let handler = Handler {
                        shared: Arc::clone(&shared), channels: HashMap::new(),
                        forward: None, stop: stop.clone(), cancelled,
                    };
                    if let Ok(running) = server::run_stream(config, stream, handler).await {
                        let handle = running.handle();
                        shared.connections.lock().expect("SSH connections").push((handle, stop.clone()));
                        let _ = running.await;
                    }
                    let _ = stop.send(true);
                    shared.connections.lock().expect("SSH connections").retain(|(_, old)| !old.same_channel(&stop));
                });
            },
            Some(_) = sessions.join_next(), if !sessions.is_empty() => {}
        }
    }
    disconnect(&shared).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while sessions.join_next().await.is_some() {}
    })
    .await
    .context("private SSH sessions did not end")?;
    let jobs = std::mem::take(&mut *shared.jobs.lock().expect("SSH jobs"));
    tokio::time::timeout(Duration::from_secs(2), async {
        for job in jobs {
            job.await.context("private SSH channel panicked")?;
        }
        Ok::<(), anyhow::Error>(())
    })
    .await
    .context("private SSH channel cleanup unconfirmed")??;
    let failures = shared.failures.lock().expect("SSH failures");
    if !failures.is_empty() {
        bail!("private SSH channel failed: {}", failures.join("; "));
    }
    Ok(())
}

struct Handler {
    shared: Arc<Shared>,
    channels: HashMap<ChannelId, Channel<Msg>>,
    forward: Option<(u32, watch::Sender<bool>)>,
    stop: watch::Sender<bool>,
    cancelled: watch::Receiver<bool>,
}

impl Drop for Handler {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

impl server::Handler for Handler {
    type Error = anyhow::Error;

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &russh::keys::PublicKey,
    ) -> Result<server::Auth> {
        Ok(
            if user == "fixture" && key.key_data() == self.shared.client_key.key_data() {
                server::Auth::Accept
            } else {
                server::Auth::reject()
            },
        )
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<()> {
        if self.channels.len() < CHANNEL_CAP {
            reply.accept().await;
            self.channels.insert(channel.id(), channel);
        }
        Ok(())
    }

    async fn channel_open_direct_streamlocal(
        &mut self,
        channel: Channel<Msg>,
        path: &str,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<()> {
        if Path::new(path) != self.shared.socket {
            return Ok(());
        }
        let mut socket = UnixStream::connect(&self.shared.socket).await?;
        reply.accept().await;
        let mut stream = channel.into_stream();
        let mut stop = self.cancelled.clone();
        self.shared.job(async move {
            tokio::select! {
                _ = tokio::io::copy_bidirectional(&mut socket, &mut stream) => {}
                _ = stop.changed() => {}
            }
        })
    }

    async fn exec_request(
        &mut self,
        id: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<()> {
        let text = std::str::from_utf8(data)?;
        if text.len() > 16 * 1024 {
            session.channel_failure(id)?;
            return Ok(());
        }
        let mut command = self.shared.environment.command("/bin/sh");
        command.args(["-c", text]);
        self.start_process(id, command, session)
    }

    async fn subsystem_request(
        &mut self,
        id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<()> {
        if name != "sftp" {
            session.channel_failure(id)?;
            return Ok(());
        }
        let program = ["/usr/libexec/sftp-server", "/usr/lib/openssh/sftp-server"]
            .into_iter()
            .find(|path| Path::new(path).is_file())
            .context("system SFTP server unavailable")?;
        let mut command = self.shared.environment.command(program);
        command.args(["-d"]).arg(&self.shared.environment.home);
        self.start_process(id, command, session)
    }

    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        session: &mut Session,
    ) -> Result<bool> {
        if address != "127.0.0.1" || *port != 0 || self.forward.is_some() {
            return Ok(false);
        }
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        *port = listener.local_addr()?.port().into();
        let forward_port = *port;
        let handle = session.handle();
        let shared = Arc::clone(&self.shared);
        let mut stop = self.cancelled.clone();
        let (cancel, mut cancelled) = watch::channel(false);
        self.forward = Some((forward_port, cancel));
        self.shared.job(async move {
            loop {
                tokio::select! {
                    _ = stop.changed() => return,
                    _ = cancelled.changed() => return,
                    accepted = listener.accept() => {
                        let Ok((mut socket, peer)) = accepted else { return };
                        let handle = handle.clone();
                        let mut stop = stop.clone();
                        let _ = shared.job(async move {
                            tokio::select! {
                                _ = stop.changed() => {},
                                channel = handle.channel_open_forwarded_tcpip("127.0.0.1", forward_port, "127.0.0.1", peer.port().into()) => {
                                    if let Ok(channel) = channel {
                                        let mut stream = channel.into_stream();
                                        tokio::select! {
                                            _ = stop.changed() => {},
                                            _ = tokio::io::copy_bidirectional(&mut socket, &mut stream) => {}
                                        }
                                    }
                                }
                            }
                        });
                    }
                }
            }
        })?;
        Ok(true)
    }

    async fn cancel_tcpip_forward(
        &mut self,
        address: &str,
        port: u32,
        _session: &mut Session,
    ) -> Result<bool> {
        if address != "127.0.0.1" || self.forward.as_ref().is_none_or(|(old, _)| *old != port) {
            return Ok(false);
        }
        let (_, cancel) = self.forward.take().expect("forward");
        let _ = cancel.send(true);
        Ok(true)
    }
}

impl Handler {
    fn start_process(
        &mut self,
        id: ChannelId,
        mut command: Command,
        session: &mut Session,
    ) -> Result<()> {
        let channel = self
            .channels
            .remove(&id)
            .context("SSH exec without session channel")?;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = OwnedChild::spawn(&mut command)?;
        session.channel_success(id)?;
        let handle = session.handle();
        let cancelled = self.cancelled.clone();
        let shared = Arc::clone(&self.shared);
        self.shared.job(async move {
            if let Err(error) = process(child, channel, handle, cancelled).await {
                let mut failures = shared.failures.lock().expect("SSH failures");
                if failures.len() < 16 {
                    failures.push(error.to_string());
                }
            }
        })
    }
}

fn pipe(pipe: impl Into<OwnedFd>) -> Result<tokio::io::unix::AsyncFd<File>> {
    let file = File::from(pipe.into());
    // These pipes belong solely to the fixture's child. AsyncFd requires
    // nonblocking descriptors, and the kernel preserves the existing flags.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(tokio::io::unix::AsyncFd::new(file)?)
}

async fn read_pipe(
    pipe: &tokio::io::unix::AsyncFd<File>,
    buffer: &mut [u8],
) -> std::io::Result<usize> {
    loop {
        let mut ready = pipe.readable().await?;
        if let Ok(result) = ready.try_io(|pipe| {
            let mut file = pipe.get_ref();
            file.read(buffer)
        }) {
            return result;
        }
    }
}

async fn write_pipe(
    pipe: &tokio::io::unix::AsyncFd<File>,
    mut buffer: &[u8],
) -> std::io::Result<()> {
    while !buffer.is_empty() {
        let mut ready = pipe.writable().await?;
        if let Ok(result) = ready.try_io(|pipe| {
            let mut file = pipe.get_ref();
            file.write(buffer)
        }) {
            let size = result?;
            if size == 0 {
                return Err(std::io::ErrorKind::WriteZero.into());
            }
            buffer = &buffer[size..];
        }
    }
    Ok(())
}

async fn process(
    mut child: OwnedChild,
    channel: Channel<Msg>,
    handle: server::Handle,
    mut stop: watch::Receiver<bool>,
) -> Result<()> {
    let stdin = pipe(child.take_stdin().context("SSH child stdin")?)?;
    let stdout = pipe(child.take_stdout().context("SSH child stdout")?)?;
    let stderr = pipe(child.take_stderr().context("SSH child stderr")?)?;
    let id = channel.id();
    let (mut reader, mut writer) = tokio::io::split(channel.into_stream());
    let mut pumps = Pumps(Vec::with_capacity(3));
    pumps.0.push(tokio::spawn(async move {
        let mut buffer = [0; 16 * 1024];
        while let Ok(size) = reader.read(&mut buffer).await {
            if size == 0 || write_pipe(&stdin, &buffer[..size]).await.is_err() {
                break;
            }
        }
    }));
    pumps.0.push(tokio::spawn(async move {
        let mut buffer = [0; 16 * 1024];
        while let Ok(size) = read_pipe(&stdout, &mut buffer).await {
            if size == 0 || writer.write_all(&buffer[..size]).await.is_err() {
                break;
            }
        }
    }));
    let errors_handle = handle.clone();
    pumps.0.push(tokio::spawn(async move {
        let mut buffer = [0; 16 * 1024];
        while let Ok(size) = read_pipe(&stderr, &mut buffer).await {
            if size == 0
                || errors_handle
                    .extended_data(id, 1, buffer[..size].to_vec())
                    .await
                    .is_err()
            {
                break;
            }
        }
    }));
    let mut poll = tokio::time::interval(Duration::from_millis(20));
    let status = loop {
        tokio::select! {
            _ = stop.changed() => {
                child.kill_tree()?;
                let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
                let status = loop {
                    if let Some(status) = child.try_wait()? { break status; }
                    ensure_before(deadline)?;
                    poll.tick().await;
                };
                break status;
            },
            _ = poll.tick() => if let Some(status) = child.try_wait()? { break status; }
        }
    };
    // ChannelStream closes the channel when its final half drops. Queue the
    // child's status while the input half is still owned, before aborting it.
    let _ = handle
        .exit_status_request(id, status.code().unwrap_or(1) as u32)
        .await;
    let input = pumps.0.remove(0);
    input.abort();
    let _ = input.await;
    for mut job in pumps.0.drain(..) {
        if tokio::time::timeout(Duration::from_secs(1), &mut job)
            .await
            .is_err()
        {
            job.abort();
            let _ = job.await;
        }
    }
    let _ = handle.eof(id).await;
    let _ = handle.close(id).await;
    Ok(())
}

/// Errors also cancel every pipe pump; the normal path awaits all three.
struct Pumps(Vec<tokio::task::JoinHandle<()>>);

impl Drop for Pumps {
    fn drop(&mut self) {
        for job in &self.0 {
            job.abort();
        }
    }
}

fn ensure_before(deadline: tokio::time::Instant) -> Result<()> {
    if tokio::time::Instant::now() >= deadline {
        bail!("private SSH child cleanup unconfirmed");
    }
    Ok(())
}
