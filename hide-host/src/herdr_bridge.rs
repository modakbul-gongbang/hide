//! The Herdr of a node that dialed its core, as that core reaches it (PRD
//! core-host-node-remote-core D-18, D-19): each stream the core opens with
//! `herdr_open` is one local connection to this node's own Herdr socket, its
//! bytes carried by the link both ways. Nothing on the core's machine listens
//! for this Herdr; the link is the only way in, and it ends every stream
//! when it ends.
//!
//! A stream is a byte pipe and nothing here reads what it carries. At most
//! [`MAX_HERDR_STREAMS`] are open at once; the next open is refused, and the
//! refusal goes to the core, which logs it with the node.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread::Scope;

use base64::Engine as _;
use hide_node_link::panes::{MAX_CHUNK, MAX_HERDR_STREAMS, NodeEvent};
use hide_platform::ipc::{LocalStream, ShutdownHandle};
use serde_json::Value;

use crate::error::{ErrorCode, HostError, HostResult};
use crate::panes::write_event;

/// How long one write to the node's Herdr may take.
const WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

struct Open {
    writer: LocalStream,
    shutdown: ShutdownHandle,
}

/// The Herdr streams of one node link.
pub struct HerdrBridge {
    socket: PathBuf,
    streams: Mutex<HashMap<u64, Open>>,
}

impl HerdrBridge {
    /// A bridge to the Herdr listening at `socket`, this node's own.
    pub fn new(socket: PathBuf) -> Self {
        Self {
            socket,
            streams: Mutex::new(HashMap::new()),
        }
    }

    /// The socket of this node's own Herdr, the one server it bridges.
    pub fn socket(&self) -> &std::path::Path {
        &self.socket
    }

    /// Connects `stream` to this node's Herdr and reads it on `scope` until
    /// either side ends it.
    pub fn open<'scope, 'env, W: Write + Send>(
        &'scope self,
        scope: &'scope Scope<'scope, 'env>,
        output: &'scope Mutex<W>,
        stream: u64,
    ) -> HostResult<Value> {
        let admitted = |streams: &HashMap<u64, Open>| {
            if streams.contains_key(&stream) {
                return Err(HostError::new(
                    ErrorCode::InvalidRequest,
                    "A Herdr stream with this id is still open",
                ));
            }
            if streams.len() >= MAX_HERDR_STREAMS {
                return Err(HostError::new(
                    ErrorCode::Busy,
                    format!("The node already has {MAX_HERDR_STREAMS} Herdr streams open"),
                ));
            }
            Ok(())
        };
        admitted(&lock(&self.streams))?;
        // Connected outside the table's lock, so a Herdr slow to accept
        // holds only this open, never another stream's write or close.
        let connection = LocalStream::connect(&self.socket).map_err(|error| {
            let code = if error.kind() == io::ErrorKind::NotFound {
                ErrorCode::NotFound
            } else {
                ErrorCode::Io
            };
            HostError::new(code, format!("The node's Herdr did not answer: {error}"))
        })?;
        // A Herdr that stops reading fails the write rather than hold a
        // worker of the link's control lane.
        connection
            .set_write_timeout(Some(WRITE_TIMEOUT))
            .map_err(|error| HostError::new(ErrorCode::Io, error.to_string()))?;
        let mut reader = connection.duplicate();
        {
            let mut streams = lock(&self.streams);
            if let Err(error) = admitted(&streams) {
                connection.shutdown_handle().shutdown();
                return Err(error);
            }
            streams.insert(
                stream,
                Open {
                    shutdown: connection.shutdown_handle(),
                    writer: connection,
                },
            );
        }
        scope.spawn(move || {
            let mut buffer = vec![0_u8; MAX_CHUNK];
            let reason = loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break "herdr_closed",
                    Ok(read) => {
                        let data =
                            base64::engine::general_purpose::STANDARD.encode(&buffer[..read]);
                        if write_event(output, &NodeEvent::HerdrData { stream, data }).is_err() {
                            break "link_closed";
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break "herdr_read_failed",
                }
            };
            // A stream the core closed is already gone from the map, and the
            // core heard nothing more of it on purpose.
            if lock(&self.streams).remove(&stream).is_some() {
                let _ = write_event(
                    output,
                    &NodeEvent::HerdrClosed {
                        stream,
                        reason: reason.to_owned(),
                    },
                );
            }
        });
        Ok(Value::Null)
    }

    /// Writes base64 `data` to `stream`'s Herdr connection.
    pub fn write(&self, stream: u64, data: &str) -> HostResult<Value> {
        if data.len() > MAX_CHUNK.div_ceil(3) * 4 {
            return Err(HostError::new(
                ErrorCode::TooLarge,
                "A Herdr chunk is larger than the link carries",
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| {
                HostError::new(ErrorCode::InvalidRequest, "A Herdr chunk is not base64")
            })?;
        let mut writer = lock(&self.streams)
            .get(&stream)
            .map(|open| open.writer.duplicate())
            .ok_or_else(|| HostError::new(ErrorCode::NotFound, "The Herdr stream has ended"))?;
        writer.write_all(&bytes).map_err(|error| {
            self.close(stream);
            HostError::new(ErrorCode::Io, error.to_string())
        })?;
        Ok(Value::Null)
    }

    /// Ends `stream` from the core's side.
    pub fn close(&self, stream: u64) {
        if let Some(open) = lock(&self.streams).remove(&stream) {
            open.shutdown.shutdown();
        }
    }

    /// The link is gone: every stream ends, and its reader with it.
    pub fn stop(&self) {
        for (_, open) in lock(&self.streams).drain() {
            open.shutdown.shutdown();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
