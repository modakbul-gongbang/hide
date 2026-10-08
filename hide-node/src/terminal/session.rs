//! One official Herdr terminal session: the `herdr terminal session` child
//! this node starts for a pane, its reader and its writer. Control is
//! writable; observe is concurrent and read-only. Dropping a session tells
//! Herdr it is released and ends only this client process (D-20).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Sender, TryRecvError, channel};
use std::thread;
use std::time::Duration;

use hide_platform::process::OwnedChild;
use serde_json::json;

use super::protocol::{self, Mode, SessionEvent};

/// Opens a pane's session; the node's own Herdr in production, a stand-in
/// in tests.
pub trait Attacher: Send + Sync {
    fn open(&self, pane_id: &str, mode: Mode, rows: u16, cols: u16)
    -> Result<SessionParts, String>;
}

/// What an open session hands the node: its output, its input in control
/// mode, and what ends it.
pub struct SessionParts {
    pub reader: Box<dyn Read + Send>,
    pub writer: Option<Box<dyn Write + Send>>,
    pub cleanup: Cleanup,
}

pub enum Cleanup {
    Child(OwnedChild),
    Other(Box<dyn FnOnce() + Send>),
    None,
}

/// The node's Herdr, reached by the CLI `herdr terminal session` client.
pub struct LocalAttacher {
    herdr_bin: Option<PathBuf>,
    socket_path: PathBuf,
}

impl LocalAttacher {
    pub fn new(herdr_bin: Option<PathBuf>, socket_path: PathBuf) -> Self {
        Self {
            herdr_bin,
            socket_path,
        }
    }
}

impl Attacher for LocalAttacher {
    fn open(
        &self,
        pane_id: &str,
        mode: Mode,
        rows: u16,
        cols: u16,
    ) -> Result<SessionParts, String> {
        let Some(herdr_bin) = self.herdr_bin.as_ref() else {
            return Err(
                "herdr binary was not found; install herdr or set its path in the app options"
                    .to_owned(),
            );
        };
        let mut command = Command::new(herdr_bin);
        // Both modes get a stdin pipe: an observer never writes to it, but
        // its end closing when this process dies, however it dies, is what
        // ends the child (B20).
        command
            .args(protocol::session_arguments(mode, pane_id, rows, cols))
            .env("HERDR_SOCKET_PATH", &self.socket_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = OwnedChild::spawn(&mut command).map_err(|error| {
            format!(
                "herdr terminal session {} could not be spawned: {error}",
                mode.as_str()
            )
        })?;
        let stdin = child
            .take_stdin()
            .ok_or_else(|| "terminal session stdin was not piped".to_owned())?;
        let reader = child
            .take_stdout()
            .ok_or_else(|| "terminal session stdout was not piped".to_owned())?;
        let writer: Option<Box<dyn Write + Send>> = match mode {
            Mode::Control => Some(Box::new(stdin)),
            // Kept open, never written, for the child's whole life.
            Mode::Observe => Some(Box::new(ObserverStdin { _stdin: stdin })),
        };
        Ok(SessionParts {
            reader: Box::new(reader),
            writer,
            cleanup: Cleanup::Child(child),
        })
    }
}

/// An observer's stdin: held so its end closes with this process, refused
/// for any write.
struct ObserverStdin {
    _stdin: std::process::ChildStdin,
}

impl Write for ObserverStdin {
    fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("an observer takes no input"))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) enum WriterCommand {
    Scroll {
        lines: i32,
        column: Option<u16>,
        row: Option<u16>,
        modifiers: u8,
    },
    Line(String),
    Release {
        acknowledged: Sender<()>,
    },
}

/// A session the node holds for a pane.
pub(super) struct Session {
    pub(super) generation: u64,
    pub(super) mode: Mode,
    writer: Option<Sender<WriterCommand>>,
    /// An observer's kept stdin, closed with the session.
    _observer_stdin: Option<Box<dyn Write + Send>>,
    cleanup: Cleanup,
    pane_id: String,
}

impl Session {
    /// Starts the writer for `parts` and returns the session and its reader,
    /// which the caller starts once the session is installed. A writer that
    /// fails says so through `on_write_failure`.
    pub(super) fn start(
        pane_id: &str,
        generation: u64,
        mode: Mode,
        parts: SessionParts,
        on_write_failure: Box<dyn Fn(String) + Send>,
    ) -> Result<(Self, Box<dyn Read + Send>), String> {
        let SessionParts {
            reader,
            writer,
            cleanup,
        } = parts;
        let (writer, observer_stdin) = match (mode, writer) {
            (Mode::Control, Some(stdin)) => {
                (Some(spawn_writer(pane_id, stdin, on_write_failure)?), None)
            }
            (Mode::Control, None) => {
                run_cleanup(cleanup, pane_id);
                return Err("terminal control stream has no writer".to_owned());
            }
            (Mode::Observe, stdin) => (None, stdin),
        };
        Ok((
            Self {
                generation,
                mode,
                writer,
                _observer_stdin: observer_stdin,
                cleanup,
                pane_id: pane_id.to_owned(),
            },
            reader,
        ))
    }

    fn send(&self, command: WriterCommand) -> Result<(), String> {
        let Some(writer) = self.writer.as_ref() else {
            return Err(format!(
                "Pane {} is read-only because another client owns terminal control",
                self.pane_id
            ));
        };
        writer
            .send(command)
            .map_err(|_| "terminal control input channel is closed".to_owned())
    }

    pub(super) fn write(&self, bytes: &[u8]) -> Result<(), String> {
        self.send(WriterCommand::Line(protocol::input_line(bytes)))
    }

    pub(super) fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        self.send(WriterCommand::Line(protocol::resize_line(rows, cols)?))
    }

    pub(super) fn scroll(
        &self,
        lines: i32,
        column: Option<u16>,
        row: Option<u16>,
        modifiers: u8,
    ) -> Result<(), String> {
        self.send(WriterCommand::Scroll {
            lines,
            column,
            row,
            modifiers,
        })
    }
}

fn spawn_writer(
    pane_id: &str,
    mut stdin: Box<dyn Write + Send>,
    on_failure: Box<dyn Fn(String) + Send>,
) -> Result<Sender<WriterCommand>, String> {
    let (sender, receiver) = channel::<WriterCommand>();
    thread::Builder::new()
        .name(format!("hide-node-terminal-writer-{pane_id}"))
        .spawn(move || {
            let mut carried = None;
            loop {
                let command = match carried.take() {
                    Some(command) => command,
                    None => match receiver.recv() {
                        Ok(command) => command,
                        Err(_) => return,
                    },
                };
                let (line, acknowledgement) = match command {
                    WriterCommand::Scroll {
                        mut lines,
                        mut column,
                        mut row,
                        mut modifiers,
                    } => {
                        // The screen has already turned precise trackpad
                        // movement into whole rows. Only wheels waiting in
                        // the channel right now are combined; nothing here
                        // waits for a frame or a timer.
                        let mut disconnected = false;
                        loop {
                            match receiver.try_recv() {
                                Ok(WriterCommand::Scroll {
                                    lines: next,
                                    column: next_column,
                                    row: next_row,
                                    modifiers: next_modifiers,
                                }) => {
                                    lines = lines.saturating_add(next);
                                    column = next_column;
                                    row = next_row;
                                    modifiers = next_modifiers;
                                }
                                Ok(command) => {
                                    carried = Some(command);
                                    break;
                                }
                                Err(TryRecvError::Empty) => break,
                                Err(TryRecvError::Disconnected) => {
                                    disconnected = true;
                                    break;
                                }
                            }
                        }
                        match protocol::scroll_line(lines, column, row, modifiers) {
                            Some(line) => (line, None),
                            None if disconnected => return,
                            None => continue,
                        }
                    }
                    WriterCommand::Line(line) => (line, None),
                    WriterCommand::Release { acknowledged } => {
                        (protocol::release_line(), Some(acknowledged))
                    }
                };
                let result = stdin
                    .write_all(line.as_bytes())
                    .and_then(|()| stdin.flush());
                if let Some(acknowledgement) = acknowledgement {
                    // A release is the session letting go; a child that
                    // already left has nothing to be told and no failure
                    // to report.
                    let _ = acknowledgement.send(());
                    return;
                }
                if let Err(error) = result {
                    on_failure(format!("terminal control write failed: {error}"));
                    return;
                }
            }
        })
        .map_err(|error| format!("terminal control writer could not be started: {error}"))?;
    Ok(sender)
}

/// Reads `reader` until the session ends, handing each event to `on_event`,
/// which answers whether to keep reading. The stream's end, a malformed line
/// and a read failure are each the session closing, with the reason.
pub(super) fn spawn_reader(
    pane_id: &str,
    mode: Mode,
    reader: Box<dyn Read + Send>,
    mut on_event: impl FnMut(SessionEvent) -> bool + Send + 'static,
) -> Result<(), String> {
    thread::Builder::new()
        .name(format!("hide-node-terminal-{}-{pane_id}", mode.as_str()))
        .spawn(move || {
            let mut lines = BufReader::new(reader).lines();
            loop {
                let event = match lines.next() {
                    None => SessionEvent::Closed { reason: None },
                    Some(Ok(line)) => match protocol::parse_line(&line) {
                        Ok(event) => event,
                        Err(message) => SessionEvent::Closed {
                            reason: Some(message),
                        },
                    },
                    Some(Err(error)) => SessionEvent::Closed {
                        reason: Some(format!("terminal session stream failed: {error}")),
                    },
                };
                let closed = matches!(event, SessionEvent::Closed { .. });
                if !on_event(event) || closed {
                    return;
                }
            }
        })
        .map(|_| ())
        .map_err(|error| format!("terminal session reader could not be started: {error}"))
}

impl Drop for Session {
    fn drop(&mut self) {
        let acknowledgement = self.writer.take().and_then(|writer| {
            let (acknowledged, acknowledgement) = channel();
            writer
                .send(WriterCommand::Release { acknowledged })
                .ok()
                .map(|()| acknowledgement)
        });
        let cleanup = std::mem::replace(&mut self.cleanup, Cleanup::None);
        if matches!(cleanup, Cleanup::None) {
            return;
        }
        let pane_id = self.pane_id.clone();
        if let Err(error) = thread::Builder::new()
            .name(format!("hide-node-terminal-reaper-{pane_id}"))
            .spawn(move || {
                if let Some(acknowledgement) = acknowledgement
                    && acknowledgement
                        .recv_timeout(Duration::from_secs(1))
                        .is_err()
                {
                    crate::diagnostic!(json!({
                        "component": "terminal_session",
                        "kind": "terminal.release_unacknowledged",
                        "pane_id": pane_id,
                    }));
                }
                run_cleanup(cleanup, &pane_id);
            })
        {
            crate::diagnostic!(json!({
                "component": "terminal_session",
                "kind": "terminal.session_reaper_spawn_failed",
                "message": error.to_string(),
            }));
        }
    }
}

fn run_cleanup(cleanup: Cleanup, pane_id: &str) {
    match cleanup {
        Cleanup::Child(mut child) => reap_child(&mut child, pane_id),
        Cleanup::Other(shutdown) => shutdown(),
        Cleanup::None => {}
    }
}

#[allow(clippy::disallowed_methods)] // a production wait, not test code
fn reap_child(child: &mut OwnedChild, pane_id: &str) {
    for _ in 0..20 {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                crate::diagnostic!(json!({
                    "component": "terminal_session",
                    "kind": "terminal.session_status_failed",
                    "pane_id": pane_id,
                    "message": error.to_string(),
                }));
                break;
            }
        }
    }
    if let Err(error) = child.kill_tree() {
        crate::diagnostic!(json!({
            "component": "terminal_session",
            "kind": "terminal.session_kill_failed",
            "pane_id": pane_id,
            "message": error.to_string(),
        }));
    }
    if let Err(error) = child.wait() {
        crate::diagnostic!(json!({
            "component": "terminal_session",
            "kind": "terminal.session_wait_failed",
            "pane_id": pane_id,
            "message": error.to_string(),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::mpsc::Receiver;

    /// A pipe end that hands each flushed batch of lines to the test.
    struct Sink {
        pending: Vec<u8>,
        flushed: Sender<Vec<serde_json::Value>>,
    }

    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.pending.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            let lines = std::mem::take(&mut self.pending);
            let lines = String::from_utf8(lines)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let _ = self.flushed.send(lines);
            Ok(())
        }
    }

    fn writer() -> (Sender<WriterCommand>, Receiver<Vec<serde_json::Value>>) {
        let (flushed, received) = channel();
        let writer = spawn_writer(
            "fixture:p1",
            Box::new(Sink {
                pending: Vec::new(),
                flushed,
            }),
            Box::new(|_| {}),
        )
        .unwrap();
        (writer, received)
    }

    fn wheel(lines: i32, column: u16) -> WriterCommand {
        WriterCommand::Scroll {
            lines,
            column: Some(column),
            row: Some(12),
            modifiers: 2,
        }
    }

    #[test]
    fn first_wheel_reaches_the_pipe_without_waiting_for_a_frame() {
        let (writer, received) = writer();
        writer.send(wheel(3, 24)).unwrap();
        let lines = received.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(lines[0]["type"], "terminal.scroll");
        assert_eq!(lines[0]["lines"], 3);
    }

    #[test]
    fn next_wheel_reaches_the_pipe_without_waiting_for_a_terminal_frame() {
        let (writer, received) = writer();
        writer.send(wheel(3, 24)).unwrap();
        received.recv_timeout(Duration::from_secs(1)).unwrap();
        writer.send(wheel(2, 25)).unwrap();
        let lines = received
            .recv_timeout(Duration::from_secs(1))
            .expect("a later wheel must not wait for an unrelated terminal frame");
        assert_eq!(lines[0]["lines"], 2);
        assert_eq!(lines[0]["column"], 25);
    }

    #[test]
    fn keyboard_input_keeps_its_order_after_scroll_input() {
        let (writer, received) = writer();
        writer.send(wheel(3, 24)).unwrap();
        received.recv_timeout(Duration::from_secs(1)).unwrap();
        writer.send(wheel(2, 25)).unwrap();
        writer
            .send(WriterCommand::Line(protocol::input_line(b"x")))
            .unwrap();
        let scroll = received.recv_timeout(Duration::from_secs(1)).unwrap();
        let input = received.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(scroll[0]["type"], "terminal.scroll");
        assert_eq!(input[0]["type"], "terminal.input");
    }

    #[test]
    fn a_failed_write_is_reported_once() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("gone"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (failed, failures) = channel();
        let failed = Mutex::new(failed);
        let writer = spawn_writer(
            "fixture:p1",
            Box::new(Broken),
            Box::new(move |message| {
                let _ = failed.lock().unwrap().send(message);
            }),
        )
        .unwrap();
        writer
            .send(WriterCommand::Line(protocol::input_line(b"x")))
            .unwrap();
        let _ = writer.send(WriterCommand::Line(protocol::input_line(b"y")));
        failures
            .recv_timeout(Duration::from_secs(1))
            .expect("the failure is reported");
        // The writer ended with the first failure, closing its end.
        assert!(failures.recv_timeout(Duration::from_secs(1)).is_err());
    }
}
