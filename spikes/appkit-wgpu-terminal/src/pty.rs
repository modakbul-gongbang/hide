use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result, anyhow};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

const MAX_TRANSCRIPT_BYTES: usize = 24 * 1024;

#[derive(Clone, Debug)]
pub struct ChildEnvironmentContract {
    pub key: &'static str,
    pub value: &'static str,
    pub requirement: &'static str,
    pub missing_behavior: &'static str,
}

pub const CHILD_ENVIRONMENT: &[ChildEnvironmentContract] = &[
    ChildEnvironmentContract {
        key: "TERM",
        value: "xterm-256color",
        requirement: "required",
        missing_behavior: "terminal capabilities would be ambiguous, so the child is not spawned",
    },
    ChildEnvironmentContract {
        key: "COLORTERM",
        value: "truecolor",
        requirement: "optional with an explicit fixed value",
        missing_behavior: "the spike still renders but the child cannot advertise true color",
    },
    ChildEnvironmentContract {
        key: "LANG",
        value: "en_US.UTF-8",
        requirement: "required",
        missing_behavior: "Korean UTF-8 round-trip cannot be claimed, so the child is not spawned",
    },
];

#[derive(Default, Debug)]
pub struct TerminalSnapshot {
    pub transcript: String,
    pub status: String,
    pub dirty_generation: u64,
}

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    snapshot: Arc<Mutex<TerminalSnapshot>>,
}

impl PtySession {
    pub fn spawn() -> Result<Self> {
        validate_environment_contract()?;

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 30,
                cols: 100,
                pixel_width: 1000,
                pixel_height: 600,
            })
            .context("stage=pty.open target=local-shell retryable=true")?;

        let mut command = CommandBuilder::new("/bin/zsh");
        command.arg("-f");
        for item in CHILD_ENVIRONMENT {
            command.env(item.key, item.value);
        }

        let child = pair
            .slave
            .spawn_command(command)
            .context("stage=pty.spawn target=/bin/zsh retryable=true")?;
        drop(pair.slave);

        let writer = Arc::new(Mutex::new(
            pair.master
                .take_writer()
                .context("stage=pty.writer target=master retryable=false")?,
        ));
        let mut reader = pair
            .master
            .try_clone_reader()
            .context("stage=pty.reader target=master retryable=false")?;
        let snapshot = Arc::new(Mutex::new(TerminalSnapshot {
            status: "PTY connected".to_owned(),
            ..TerminalSnapshot::default()
        }));
        let reader_snapshot = Arc::clone(&snapshot);

        thread::Builder::new()
            .name("herdr-spike-pty-reader".to_owned())
            .spawn(move || {
                let mut bytes = [0_u8; 4096];
                loop {
                    match reader.read(&mut bytes) {
                        Ok(0) => {
                            update_status(&reader_snapshot, "PTY child exited");
                            break;
                        }
                        Ok(count) => {
                            let text = String::from_utf8_lossy(&bytes[..count]);
                            let mut state = reader_snapshot
                                .lock()
                                .expect("terminal snapshot lock poisoned");
                            state.transcript.push_str(&text);
                            if state.transcript.len() > MAX_TRANSCRIPT_BYTES {
                                let start = state.transcript.len() - MAX_TRANSCRIPT_BYTES;
                                let boundary = state.transcript.ceil_char_boundary(start);
                                state.transcript.drain(..boundary);
                            }
                            state.dirty_generation += 1;
                        }
                        Err(error) => {
                            update_status(&reader_snapshot, &format!("PTY reader failed: {error}"));
                            break;
                        }
                    }
                }
            })
            .context("stage=pty.reader-thread target=local retryable=false")?;

        let session = Self {
            master: pair.master,
            child,
            writer,
            snapshot,
        };
        session.write_text("printf 'real PTY ready - UTF-8 한글 경로 대기\\r\\n'\r")?;
        Ok(session)
    }

    pub fn snapshot(&self) -> Arc<Mutex<TerminalSnapshot>> {
        Arc::clone(&self.snapshot)
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    pub fn write_text(&self, text: &str) -> Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| anyhow!("stage=pty.write target=master cause=poisoned-lock"))?;
        writer
            .write_all(text.as_bytes())
            .context("stage=pty.write target=master retryable=true")?;
        writer
            .flush()
            .context("stage=pty.flush target=master retryable=true")
    }

    pub fn resize(&self, width: u32, height: u32) -> Result<()> {
        let cols = (width / 9).clamp(20, u16::MAX as u32) as u16;
        let rows = (height / 18).clamp(5, u16::MAX as u32) as u16;
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: width.min(u16::MAX as u32) as u16,
                pixel_height: height.min(u16::MAX as u32) as u16,
            })
            .context("stage=pty.resize target=master retryable=true")
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        if let Err(error) = self.child.kill() {
            eprintln!("event=pty.cleanup.failed stage=child.kill retryable=false error={error:?}");
        }
    }
}

fn validate_environment_contract() -> Result<()> {
    for item in CHILD_ENVIRONMENT {
        if item.requirement == "required" && item.value.is_empty() {
            return Err(anyhow!(
                "stage=pty.environment target={} cause=empty-required-value behavior={}",
                item.key,
                item.missing_behavior
            ));
        }
    }
    Ok(())
}

fn update_status(snapshot: &Arc<Mutex<TerminalSnapshot>>, status: &str) {
    if let Ok(mut state) = snapshot.lock() {
        state.status = status.to_owned();
        state.dirty_generation += 1;
    } else {
        eprintln!("event=pty.status.failed stage=terminal-snapshot cause=poisoned-lock");
    }
}

pub fn visible_transcript(raw: &str, max_lines: usize) -> String {
    let cleaned = strip_ansi(raw).replace('\r', "");
    let lines: Vec<&str> = cleaned.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

fn strip_ansi(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\u{1b}' {
            result.push(character);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_boundary_preserves_unicode_and_removes_control_sequences() {
        let raw = "\u{1b}[31m오류\u{1b}[0m\r\n정상\n완료";
        assert_eq!(visible_transcript(raw, 2), "정상\n완료");
    }

    #[test]
    fn environment_contract_is_enumerable_and_required_values_are_nonempty() {
        assert!(CHILD_ENVIRONMENT.len() >= 3);
        assert!(validate_environment_contract().is_ok());
        assert!(CHILD_ENVIRONMENT.iter().all(|item| !item.key.is_empty()));
    }
}
