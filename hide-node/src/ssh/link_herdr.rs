//! The Herdr of a node that dialed its core, reached through the node's
//! link (PRD core-host-node-remote-core D-18, D-19): an [`ApiConnector`]
//! whose every connection is one stream inside the link, which the node
//! bridges to its own Herdr socket (`hide_host::herdr_bridge`). The session
//! sync, controls, doorbell, find, phone reply and agent sleep take it where
//! they take a local socket or an SSH device's channel.
//!
//! A stream reads what the node sent, in order, from a bounded queue: one
//! the reader leaves past [`MAX_HERDR_PENDING`] chunks is ended as stalled,
//! and a link holds at most [`MAX_HERDR_STREAMS`] at once. The link ending
//! ends every stream on it.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use base64::Engine as _;
use hide_herdr_client::{ApiConnector, ApiError, ApiStream, ConnectionShutdown};
use hide_node_link::panes::{MAX_CHUNK, MAX_HERDR_STREAMS};
use hide_node_link::protocol::Call;
use serde_json::json;

use super::{RemoteHost, lock_recover};

/// Chunks one Herdr stream holds for its reader; past it the stream ends.
pub const MAX_HERDR_PENDING: usize = 64;
/// How long an open, a write or a close may take on the link.
const HERDR_CALL_TIMEOUT: Duration = Duration::from_secs(5);

enum Piece {
    Data(Vec<u8>),
    Ended(String),
}

/// The Herdr streams open on one link, by the id the core gave each.
#[derive(Default)]
pub(super) struct HerdrStreams {
    open: Mutex<HashMap<u64, mpsc::SyncSender<Piece>>>,
    next: AtomicU64,
}

impl HerdrStreams {
    /// Bytes the node read from its Herdr for `stream`. A reader that has
    /// fallen [`MAX_HERDR_PENDING`] chunks behind loses its stream.
    pub(super) fn data(&self, target: &str, stream: u64, data: &str) {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
            self.end(stream, "the node sent a Herdr chunk that is not base64");
            return;
        };
        let mut open = lock_recover(&self.open);
        let Some(sender) = open.get(&stream) else {
            return;
        };
        match sender.try_send(Piece::Data(bytes)) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                let sender = open.remove(&stream);
                drop(open);
                crate::diagnostic!(json!({
                    "component": "remote_host",
                    "kind": "herdr_stream.overflow",
                    "target": target,
                    "stream": stream,
                    "cap": MAX_HERDR_PENDING,
                }));
                if let Some(sender) = sender {
                    // The reader finds the queue's end after what it holds.
                    drop(sender);
                }
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                open.remove(&stream);
            }
        }
    }

    /// The node ended `stream`, or the core gave it up.
    pub(super) fn end(&self, stream: u64, reason: &str) {
        if let Some(sender) = lock_recover(&self.open).remove(&stream) {
            let _ = sender.try_send(Piece::Ended(reason.to_owned()));
        }
    }

    /// The link ended: every stream reads its end.
    pub(super) fn end_all(&self) {
        lock_recover(&self.open).clear();
    }
}

/// Connects to the Herdr of the node at the other end of `link`.
#[derive(Clone)]
pub struct LinkHerdrConnector {
    link: RemoteHost,
}

impl std::fmt::Debug for LinkHerdrConnector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LinkHerdrConnector")
            .field("target", &self.link.target())
            .finish()
    }
}

impl RemoteHost {
    /// The Herdr of the node at the other end of this link, reached through
    /// it: a node that dialed its core (PRD core-host-node-remote-core D-18).
    pub fn herdr_connector(&self) -> LinkHerdrConnector {
        LinkHerdrConnector { link: self.clone() }
    }
}

impl ApiConnector for LinkHerdrConnector {
    fn connect(&self) -> Result<Box<dyn ApiStream>, ApiError> {
        let streams = &self.link.inner.herdr;
        if let Some(reason) = self.link.closed_reason() {
            return Err(ApiError::Transport(format!(
                "the node's link has ended ({reason})"
            )));
        }
        let stream = streams.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (sender, receiver) = mpsc::sync_channel(MAX_HERDR_PENDING);
        {
            let mut open = lock_recover(&streams.open);
            if open.len() >= MAX_HERDR_STREAMS {
                drop(open);
                crate::diagnostic!(json!({
                    "component": "remote_host",
                    "kind": "herdr_stream.cap_reached",
                    "target": self.link.target(),
                    "cap": MAX_HERDR_STREAMS,
                }));
                return Err(ApiError::Transport(format!(
                    "the node's link already carries {MAX_HERDR_STREAMS} Herdr streams"
                )));
            }
            open.insert(stream, sender);
        }
        if let Err(error) = self
            .link
            .call(Call::HerdrOpen { stream }, HERDR_CALL_TIMEOUT)
        {
            lock_recover(&streams.open).remove(&stream);
            let message = format!("the node's Herdr could not be reached: {error}");
            return Err(match &error {
                hide_node_link::LinkError::Refused(refusal)
                    if refusal.code == hide_node_link::ErrorCode::NotFound =>
                {
                    ApiError::NotRunning(message)
                }
                _ => ApiError::Transport(message),
            });
        }
        Ok(Box::new(LinkHerdrStream {
            link: self.link.clone(),
            stream,
            state: Arc::new(StreamState {
                receiver: Mutex::new(receiver),
                ended: Mutex::new(None),
            }),
            held: Vec::new(),
            read_timeout: Mutex::new(None),
            write_timeout: Mutex::new(None),
        }))
    }
}

struct StreamState {
    receiver: Mutex<mpsc::Receiver<Piece>>,
    ended: Mutex<Option<String>>,
}

/// One Herdr connection of a linked node.
struct LinkHerdrStream {
    link: RemoteHost,
    stream: u64,
    state: Arc<StreamState>,
    /// Bytes read from a chunk and not yet taken.
    held: Vec<u8>,
    read_timeout: Mutex<Option<Duration>>,
    write_timeout: Mutex<Option<Duration>>,
}

impl LinkHerdrStream {
    /// The next chunk, waiting at most until `deadline`; `None` at the end.
    fn next_chunk(&mut self, deadline: Option<Instant>) -> io::Result<Option<Vec<u8>>> {
        if self
            .state
            .ended
            .lock()
            .map(|ended| ended.is_some())
            .unwrap_or(true)
        {
            return Ok(None);
        }
        let receiver = lock_recover(&self.state.receiver);
        let piece = match deadline {
            None => receiver.recv().map_err(|_| None),
            Some(deadline) => receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => Some(()),
                    mpsc::RecvTimeoutError::Disconnected => None,
                }),
        };
        drop(receiver);
        match piece {
            Ok(Piece::Data(bytes)) => Ok(Some(bytes)),
            Ok(Piece::Ended(reason)) => {
                *lock_recover(&self.state.ended) = Some(reason);
                Ok(None)
            }
            Err(Some(())) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the node's Herdr sent nothing in time",
            )),
            Err(None) => {
                *lock_recover(&self.state.ended) = Some("the stream ended".to_owned());
                Ok(None)
            }
        }
    }
}

impl Read for LinkHerdrStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.held.is_empty() {
            let deadline = lock_recover(&self.read_timeout).map(|timeout| Instant::now() + timeout);
            match self.next_chunk(deadline)? {
                Some(bytes) => self.held = bytes,
                None => return Ok(0),
            }
        }
        let taken = self.held.len().min(buffer.len());
        buffer[..taken].copy_from_slice(&self.held[..taken]);
        self.held.drain(..taken);
        Ok(taken)
    }
}

impl Write for LinkHerdrStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let taken = buffer.len().min(MAX_CHUNK);
        let data = base64::engine::general_purpose::STANDARD.encode(&buffer[..taken]);
        let timeout = lock_recover(&self.write_timeout).unwrap_or(HERDR_CALL_TIMEOUT);
        self.link
            .call(
                Call::HerdrWrite {
                    stream: self.stream,
                    data,
                },
                timeout,
            )
            .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error.to_string()))?;
        Ok(taken)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl ApiStream for LinkHerdrStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), ApiError> {
        *lock_recover(&self.read_timeout) = timeout;
        Ok(())
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> Result<(), ApiError> {
        *lock_recover(&self.write_timeout) = timeout;
        Ok(())
    }

    fn read_line_with_timeout(&mut self, timeout: Duration) -> Result<String, ApiError> {
        let deadline = Instant::now() + timeout;
        let mut line = Vec::new();
        loop {
            if let Some(end) = self.held.iter().position(|byte| *byte == b'\n') {
                line.extend(self.held.drain(..=end));
                break;
            }
            line.append(&mut self.held);
            if line.len() > 64 * 1024 {
                return Err(ApiError::Malformed(
                    "subscription acknowledgement exceeds 64 KiB".to_owned(),
                ));
            }
            match self.next_chunk(Some(deadline)) {
                Ok(Some(bytes)) => self.held = bytes,
                Ok(None) => {
                    return Err(ApiError::Transport(
                        "subscription acknowledgement reached EOF".to_owned(),
                    ));
                }
                Err(_) => {
                    return Err(ApiError::Transport(
                        "subscription acknowledgement timed out".to_owned(),
                    ));
                }
            }
        }
        // The reader it becomes after the acknowledgement has no deadline.
        *lock_recover(&self.read_timeout) = None;
        String::from_utf8(line).map_err(|error| {
            ApiError::Malformed(format!(
                "subscription acknowledgement was not UTF-8: {error}"
            ))
        })
    }

    fn shutdown_handle(&self) -> Result<Box<dyn ConnectionShutdown>, ApiError> {
        Ok(Box::new(LinkHerdrShutdown {
            link: self.link.clone(),
            stream: self.stream,
        }))
    }
}

impl Drop for LinkHerdrStream {
    fn drop(&mut self) {
        close_stream(&self.link, self.stream);
    }
}

struct LinkHerdrShutdown {
    link: RemoteHost,
    stream: u64,
}

impl ConnectionShutdown for LinkHerdrShutdown {
    fn shutdown(&self) {
        close_stream(&self.link, self.stream);
    }
}

/// Ends `stream` on this side at once, so its reader reads the end, and
/// tells the node, which closes its Herdr connection. Telling the node
/// waits on the link, so it is done off any runtime thread.
fn close_stream(link: &RemoteHost, stream: u64) {
    let open = lock_recover(&link.inner.herdr.open).remove(&stream);
    let Some(sender) = open else {
        return;
    };
    let _ = sender.try_send(Piece::Ended("closed by this side".to_owned()));
    if link.closed_reason().is_some() {
        return;
    }
    let link = link.clone();
    let tell = move || {
        let _ = link.call(Call::HerdrClose { stream }, HERDR_CALL_TIMEOUT);
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        let _ = std::thread::Builder::new()
            .name("herdr-stream-close".into())
            .spawn(tell);
    } else {
        tell();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hide_platform::ipc::LocalListener;
    use std::io::{BufRead, BufReader};

    /// A Herdr that answers each line it reads with the same line, until
    /// its connection ends.
    fn echo_herdr(folder: &std::path::Path) -> std::path::PathBuf {
        let socket = folder.join("h.sock");
        let listener = LocalListener::bind(&socket).expect("a Herdr socket");
        std::thread::spawn(move || {
            while let Ok(connection) = listener.accept() {
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(connection.duplicate());
                    let mut writer = connection;
                    let mut line = String::new();
                    while reader.read_line(&mut line).is_ok_and(|read| read > 0) {
                        if writer.write_all(line.as_bytes()).is_err() {
                            return;
                        }
                        line.clear();
                    }
                });
            }
        });
        socket
    }

    /// A link whose other end is a node serving `herdr_socket` as its own.
    fn linked_node(herdr_socket: std::path::PathBuf) -> RemoteHost {
        let (core_end, node_end) = hide_platform::ipc::LocalStream::pair().expect("a pair");
        std::thread::spawn(move || {
            let input = BufReader::new(node_end.duplicate());
            let _ = hide_host::serve::serve_with(
                input,
                node_end,
                hide_host::serve::Services {
                    terminals: None,
                    herdr_socket: Some(herdr_socket),
                },
            );
        });
        super::super::over_local_stream("inbound:test", core_end, None, Box::new(|_| {}))
            .expect("a local link")
    }

    /// The core reaches a linked node's Herdr through the link: a request
    /// written on a stream gets the node's Herdr's answer, line by line, and
    /// two streams do not share bytes (D-18).
    #[test]
    fn a_linked_node_s_herdr_answers_through_the_link() {
        let folder = tempfile::tempdir().unwrap();
        let link = linked_node(echo_herdr(folder.path()));
        let connector = link.herdr_connector();
        let mut first = connector.connect().expect("a first stream");
        let mut second = connector.connect().expect("a second stream");
        first.write_all(b"{\"id\":\"a\"}\n").unwrap();
        second.write_all(b"{\"id\":\"b\"}\n").unwrap();
        assert_eq!(
            first
                .read_line_with_timeout(Duration::from_secs(10))
                .unwrap(),
            "{\"id\":\"a\"}\n"
        );
        assert_eq!(
            second
                .read_line_with_timeout(Duration::from_secs(10))
                .unwrap(),
            "{\"id\":\"b\"}\n"
        );
        drop(first);
        // A read past its timeout says so rather than returning early.
        second
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let mut byte = [0_u8; 1];
        let error = second.read(&mut byte).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    /// At most `MAX_HERDR_STREAMS` streams are open on a link; the next is
    /// refused rather than queued, and one closing makes room (D-20).
    #[test]
    fn a_link_holds_at_most_its_cap_of_herdr_streams() {
        let folder = tempfile::tempdir().unwrap();
        let link = linked_node(echo_herdr(folder.path()));
        let connector = link.herdr_connector();
        let mut open: Vec<_> = (0..MAX_HERDR_STREAMS)
            .map(|_| connector.connect().expect("a stream under the cap"))
            .collect();
        let refused = connector.connect().err().expect("the stream past the cap");
        assert!(refused.to_string().contains("already carries"), "{refused}");
        open.pop();
        connector.connect().expect("room after a close");
    }

    /// The link ending ends every stream on it: a blocked reader reads the
    /// end, and a new stream is refused.
    #[test]
    fn the_link_ending_ends_its_herdr_streams() {
        let folder = tempfile::tempdir().unwrap();
        let link = linked_node(echo_herdr(folder.path()));
        let mut stream = link.herdr_connector().connect().expect("a stream");
        let reader = std::thread::spawn(move || {
            let mut byte = [0_u8; 1];
            stream.read(&mut byte)
        });
        link.close("test ended the link");
        assert_eq!(reader.join().unwrap().unwrap(), 0);
        assert!(link.herdr_connector().connect().is_err());
    }

    /// A node with no Herdr bridge, a device its core dialed, refuses a
    /// Herdr stream, and its refusal reaches the caller.
    #[test]
    fn a_node_without_a_bridge_refuses_herdr_streams() {
        let (core_end, node_end) = hide_platform::ipc::LocalStream::pair().expect("a pair");
        std::thread::spawn(move || {
            let input = BufReader::new(node_end.duplicate());
            let _ =
                hide_host::serve::serve_with(input, node_end, hide_host::serve::Services::none());
        });
        let link =
            super::super::over_local_stream("inbound:test", core_end, None, Box::new(|_| {}))
                .expect("a local link");
        let refused = link.herdr_connector().connect().err().expect("a refusal");
        assert!(
            refused.to_string().contains("not reached through its link"),
            "{refused}"
        );
    }
}
