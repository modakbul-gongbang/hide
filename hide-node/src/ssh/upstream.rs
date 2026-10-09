//! The one SSH connection a node opens to its core on another machine (PRD
//! core-host-node-remote-core D-04, D-07): an exec channel that runs the
//! core machine's attach role carries the node's link, and the screen relay
//! reaches the core's loopback port through forwards on the same connection.
//!
//! The screen machine needs no SSH server of its own, only the alias. The
//! connection checks the core machine's key against `known_hosts` exactly as
//! a device the core dials is checked, and ends when its owner drops it.

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hide_node_link::device::{RemoteResult, RemoteStage};
use hide_platform::ipc::LocalStream;
use russh::ChannelMsg;
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;

use super::{RemoteLocalForward, RusshRemoteClient, SshAlias, remote_error, shell_quote};

/// Bytes carried per read in either direction of the attach channel.
const CHUNK: usize = 64 * 1024;

/// The node's connection to its core's machine.
pub struct Upstream {
    client: RusshRemoteClient,
    /// Set while a resolution of the core machine's name runs: one at a
    /// time, however long the system's resolver takes.
    resolving: Arc<AtomicBool>,
}

/// A probe of the SSH greeting, bounded as a whole by `within`. The name is
/// resolved on a thread of its own the probe waits for only until its
/// deadline: the system's resolver has no bound, and hangs exactly when the
/// network is down, so it never holds the caller (the node's watch, which
/// the role's end joins). That thread ends when the resolver answers; while
/// one runs, a probe answers not reachable rather than start another.
fn probe<R>(within: Duration, resolving: &Arc<AtomicBool>, resolve: R) -> bool
where
    R: FnOnce() -> io::Result<Vec<SocketAddr>> + Send + 'static,
{
    let deadline = Instant::now() + within;
    if resolving.swap(true, Ordering::SeqCst) {
        return false;
    }
    let (found, answer) = mpsc::channel();
    let running = Arc::clone(resolving);
    let started = std::thread::Builder::new()
        .name("core-resolve".to_owned())
        .spawn(move || {
            let addresses = resolve();
            running.store(false, Ordering::SeqCst);
            let _ = found.send(addresses);
        });
    if started.is_err() {
        resolving.store(false, Ordering::SeqCst);
        return false;
    }
    let Ok(Ok(addresses)) = answer.recv_timeout(within) else {
        return false;
    };
    addresses.into_iter().any(|address| {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        let Ok(mut socket) = TcpStream::connect_timeout(&address, left) else {
            return false;
        };
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || socket.set_read_timeout(Some(left)).is_err() {
            return false;
        }
        let mut greeting = [0_u8; 4];
        socket.read_exact(&mut greeting).is_ok() && &greeting == b"SSH-"
    })
}

/// The attach role's channel, as a local stream: what the node writes to it
/// reaches the attach role's standard input, and the attach role's standard
/// output is what the node reads. The channel ends when the stream ends, and
/// the stream ends when the channel does.
pub struct AttachChannel {
    pub stream: LocalStream,
    /// What the attach role wrote to its standard error, for the diagnostic
    /// when the channel ends before a handshake.
    pub stderr: Arc<std::sync::Mutex<Vec<u8>>>,
}

/// The most of the attach role's standard error kept for a diagnostic.
const STDERR_CAP: usize = 4096;

impl Upstream {
    pub fn new(alias: SshAlias) -> RemoteResult<Self> {
        Ok(Self {
            client: RusshRemoteClient::new(alias)?,
            resolving: Arc::default(),
        })
    }

    /// The alias's host id, for diagnostics.
    pub fn host_id(&self) -> &str {
        &self.client.host.host_id
    }

    /// The alias this connection dials, as it was read.
    pub fn alias(&self) -> &SshAlias {
        &self.client.host
    }

    /// Dials the core's machine when the connection is not up, and runs
    /// `program attach [--state-dir <dir>]` there on a new exec channel.
    pub fn attach(&self, program: &str, state_dir: Option<&str>) -> RemoteResult<AttachChannel> {
        let target = self.client.host.host_id.clone();
        let mut command = format!("{} attach", shell_quote(program));
        if let Some(state_dir) = state_dir {
            command.push_str(" --state-dir ");
            command.push_str(&shell_quote(state_dir));
        }
        let client = &self.client;
        let channel = client.runtime.block_on(async {
            let permit = client
                .session_channel("node-attach", RemoteStage::Ssh)
                .await?;
            let session = client.shared_session().await?;
            let opened = super::bounded_ssh_operation(session.channel_open_session()).await;
            let channel = match opened {
                Ok(channel) => channel,
                Err(error) => {
                    client.forget_session(&session).await;
                    return Err(remote_error(
                        "node-attach",
                        &target,
                        RemoteStage::Ssh,
                        error,
                        true,
                        false,
                    ));
                }
            };
            super::bounded_ssh_operation(channel.exec(true, command))
                .await
                .map_err(|error| {
                    remote_error("node-attach", &target, RemoteStage::Ssh, error, true, false)
                })?;
            Ok::<_, hide_node_link::device::RemoteError>((channel, permit))
        })?;
        let (ours, theirs) = LocalStream::pair().map_err(|error| {
            remote_error(
                "node-attach",
                &target,
                RemoteStage::Ssh,
                error,
                false,
                false,
            )
        })?;
        let stderr = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (channel, permit) = channel;
        let (mut read_half, write_half) = channel.split();
        let to_node = theirs.duplicate();
        let shutdown = theirs.shutdown_handle();
        // The channel's output, to the node: data to the stream, standard
        // error to the diagnostic, and the stream ended with the channel.
        {
            let stderr = Arc::clone(&stderr);
            client.runtime.spawn(async move {
                let _permit = permit;
                let mut to_node = to_node;
                while let Some(message) = read_half.wait().await {
                    match message {
                        ChannelMsg::Data { data } => {
                            let mut to_node_now = to_node;
                            let written = tokio::task::spawn_blocking(move || {
                                let result = to_node_now.write_all(&data);
                                (to_node_now, result)
                            })
                            .await;
                            match written {
                                Ok((back, Ok(()))) => to_node = back,
                                _ => break,
                            }
                        }
                        ChannelMsg::ExtendedData { data, .. } => {
                            let mut kept = stderr.lock().unwrap_or_else(|p| p.into_inner());
                            let room = STDERR_CAP.saturating_sub(kept.len());
                            kept.extend_from_slice(&data[..data.len().min(room)]);
                        }
                        ChannelMsg::Eof | ChannelMsg::Close => break,
                        _ => {}
                    }
                }
                shutdown.shutdown();
            });
        }
        // The node's bytes, to the channel, until the node's end of the
        // stream closes; then the attach role reads the end of its input.
        let handle = client.runtime.handle().clone();
        std::thread::Builder::new()
            .name("node-attach-up".to_owned())
            .spawn(move || {
                let mut from_node = theirs;
                let mut writer = write_half.make_writer();
                let mut buffer = vec![0_u8; CHUNK];
                loop {
                    match from_node.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(read) => {
                            let sent = handle.block_on(async {
                                writer.write_all(&buffer[..read]).await?;
                                writer.flush().await
                            });
                            if sent.is_err() {
                                break;
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
                let _ = handle.block_on(async {
                    let _ = writer.shutdown().await;
                    write_half.close().await
                });
                from_node.shutdown_handle().shutdown();
            })
            .map_err(|error| {
                remote_error(
                    "node-attach",
                    &target,
                    RemoteStage::Ssh,
                    error,
                    false,
                    false,
                )
            })?;
        Ok(AttachChannel {
            stream: ours,
            stderr,
        })
    }

    /// A loopback port on this machine whose connections reach `port` on
    /// the core machine's loopback, over the same connection.
    pub fn forward(&self, port: u16) -> RemoteResult<RemoteLocalForward> {
        let (_cancel, canceled) = oneshot::channel();
        // The sender is kept alive for the call: a dropped sender reads as a
        // cancel.
        let forward = self.page_forward(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            None,
            true,
            canceled,
        );
        drop(_cancel);
        forward
    }

    /// A page's way to `remote` on the core machine's loopback, over the
    /// same connection, as a device's page route reaches its device
    /// (`start_local_workspace_forward`): `alternate` is the other address
    /// family a `localhost` page may resolve to, and a numeric HTTPS host
    /// keeps its address for the certificate.
    pub fn page_forward(
        &self,
        remote: SocketAddr,
        alternate: Option<SocketAddr>,
        preserve_numeric_host: bool,
        canceled: oneshot::Receiver<()>,
    ) -> RemoteResult<RemoteLocalForward> {
        self.client.start_local_workspace_forward(
            remote,
            alternate,
            preserve_numeric_host,
            canceled,
        )
    }

    /// Whether the connection still answers: one SSH ping, answered
    /// within `within`. A connection that is not up is not alive. Asked after
    /// the machine woke or its network changed, when a connection that looks
    /// up may lead nowhere.
    pub fn alive(&self, within: Duration) -> bool {
        let connection = &self.client.connection;
        self.client.runtime.block_on(async {
            let session = connection.session.lock().await.clone();
            let Some(session) = session.filter(|session| !session.is_closed()) else {
                return false;
            };
            let answered = tokio::time::timeout(within, session.send_ping())
                .await
                .is_ok_and(|sent| sent.is_ok());
            answered && !session.is_closed()
        })
    }

    /// Whether the core machine's SSH server answers now: its name resolved,
    /// a connection to its port, and the server's `SSH-` greeting read, all
    /// within `within`. A server that takes the connection and closes it, a
    /// port nothing answers, or a name the resolver has not answered in time
    /// is not reachable.
    pub fn reachable(&self, within: Duration) -> bool {
        let host = &self.client.host;
        let (name, port) = (host.hostname.clone(), host.port);
        probe(within, &self.resolving, move || {
            (name.as_str(), port)
                .to_socket_addrs()
                .map(|found| found.collect())
        })
    }

    /// Ends the connection and every channel on it.
    pub fn close(&self) {
        self.client.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    /// A resolver that does not answer holds the probe only until its
    /// deadline, and no second resolution starts while it runs; once it has
    /// answered, a probe reads the server's greeting again.
    #[test]
    fn a_resolver_that_hangs_holds_the_probe_only_until_its_deadline() {
        let resolving = Arc::new(AtomicBool::new(false));
        let (release, released) = mpsc::channel::<()>();
        let within = Duration::from_millis(200);
        let started = Instant::now();
        let reached = probe(within, &resolving, move || {
            let _ = released.recv_timeout(Duration::from_secs(30));
            Ok(Vec::new())
        });
        assert!(!reached);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );

        let asked = Arc::new(AtomicBool::new(false));
        let second = Arc::clone(&asked);
        assert!(!probe(within, &resolving, move || {
            second.store(true, Ordering::SeqCst);
            Ok(Vec::new())
        }));
        assert!(!asked.load(Ordering::SeqCst), "a second resolution started");

        drop(release);
        let deadline = Instant::now() + Duration::from_secs(10);
        while resolving.load(Ordering::SeqCst) {
            assert!(Instant::now() < deadline, "the resolution never ended");
            std::thread::yield_now();
        }
        let server = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = server.local_addr().unwrap();
        let greeting = std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            stream.write_all(b"SSH-2.0-fixture\r\n").unwrap();
        });
        assert!(probe(Duration::from_secs(5), &resolving, move || Ok(vec![
            address
        ])));
        greeting.join().unwrap();
    }
}
