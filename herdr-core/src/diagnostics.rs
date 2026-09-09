//! One bounded, nonblocking queue keeps diagnostic I/O out of the runtime lock.
//! The stderr line and the on-disk line are identical. Files live next to the
//! app's state in Logs, so a suffixed development instance owns separate logs.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};

const FILE_LIMIT: u64 = 1024 * 1024;
const LINE_LIMIT: usize = 64 * 1024;
static SINK: OnceLock<Mutex<Option<DiagnosticSink>>> = OnceLock::new();

#[derive(Clone)]
struct DiagnosticSink {
    sender: SyncSender<String>,
    dropped: Arc<AtomicU64>,
}

/// Emits one JSON record. The argument is a `serde_json::Value`, so a line
/// that is not JSON cannot be written; the sink never has to parse its input.
#[macro_export]
macro_rules! diagnostic {
    ($value:expr) => {
        $crate::diagnostics::emit($value)
    };
}

pub(crate) fn install(state_path: &Path) -> io::Result<()> {
    let directory = state_path
        .parent()
        .ok_or_else(|| io::Error::other("state path has no parent"))?
        .join("Logs");
    let writer = RotatingLog::open(directory, FILE_LIMIT)?;
    let (sender, receiver) = mpsc::sync_channel::<String>(512);
    let dropped = Arc::new(AtomicU64::new(0));
    let writer_dropped = Arc::clone(&dropped);
    std::thread::Builder::new()
        .name("diagnostics".into())
        .spawn(move || {
            let mut writer = writer;
            let mut write_failed = false;
            for message in receiver {
                let dropped = writer_dropped.swap(0, Ordering::Relaxed);
                let overflow = (dropped > 0).then(|| {
                    serde_json::json!({
                        "kind": "diagnostics.queue_overflow", "dropped": dropped
                    })
                    .to_string()
                });
                for raw in overflow.iter().chain(std::iter::once(&message)) {
                    let line = if raw.len() > LINE_LIMIT {
                        serde_json::json!({"kind":"diagnostics.line_too_large", "bytes":raw.len()})
                            .to_string()
                    } else {
                        raw.clone()
                    };
                    write_stderr_line(&line);
                    match writer.append(&line) {
                        Ok(()) => write_failed = false,
                        Err(error) if !write_failed => {
                            write_stderr_line(
                                &serde_json::json!({
                                    "kind": "diagnostics.write_failed", "message": error.to_string()
                                })
                                .to_string(),
                            );
                            write_failed = true;
                        }
                        Err(_) => {}
                    }
                }
            }
        })?;
    // A process normally owns one core. Replacing the active sink also makes
    // destroy-and-create cycles bind diagnostics to the new core's state path
    // instead of keeping the first path for the rest of the process lifetime.
    *sink_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
        Some(DiagnosticSink { sender, dropped });
    Ok(())
}

pub(crate) fn emit(record: serde_json::Value) {
    let message = record.to_string();
    let sink = sink_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(sink) = sink {
        if sink.sender.try_send(message).is_err() {
            sink.dropped.fetch_add(1, Ordering::Relaxed);
        }
    } else {
        write_stderr_line(&message);
    }
}

fn sink_slot() -> &'static Mutex<Option<DiagnosticSink>> {
    SINK.get_or_init(|| Mutex::new(None))
}

fn write_stderr_line(message: &str) {
    // Attach children inherit stderr, so build each complete JSONL record before
    // taking the process-local lock and writing it to the shared descriptor.
    let mut line = String::with_capacity(message.len() + 1);
    line.push_str(message);
    line.push('\n');
    let _ = io::stderr().lock().write_all(line.as_bytes());
}

struct RotatingLog {
    directory: PathBuf,
    file: File,
    size: u64,
    limit: u64,
}

impl RotatingLog {
    fn open(directory: PathBuf, limit: u64) -> io::Result<Self> {
        fs::create_dir_all(&directory)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("core.jsonl"))?;
        let size = file.metadata()?.len();
        Ok(Self {
            directory,
            file,
            size,
            limit,
        })
    }

    fn append(&mut self, line: &str) -> io::Result<()> {
        if self.size + line.len() as u64 + 1 > self.limit {
            fs::rename(
                self.directory.join("core.jsonl"),
                self.directory.join("core.previous.jsonl"),
            )?;
            self.file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(self.directory.join("core.jsonl"))?;
            self.size = 0;
        }
        writeln!(self.file, "{line}")?;
        self.size += line.len() as u64 + 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_rotate_without_splitting_json_records() {
        let directory =
            std::env::temp_dir().join(format!("hide-diagnostics-{}", std::process::id()));
        let mut log = RotatingLog::open(directory.clone(), 256).unwrap();
        for n in 0..100 {
            log.append(&serde_json::json!({"kind":"test", "n":n}).to_string())
                .unwrap();
        }
        for name in ["core.jsonl", "core.previous.jsonl"] {
            let bytes = fs::read_to_string(directory.join(name)).unwrap();
            assert!(bytes.len() <= 256);
            for line in bytes.lines() {
                assert!(serde_json::from_str::<serde_json::Value>(line).is_ok());
            }
        }
        assert!(
            fs::read_to_string(directory.join("core.jsonl"))
                .unwrap()
                .contains("99")
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
