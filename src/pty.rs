use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use alacritty_terminal::vte::ansi;
use anyhow::{Context, Result, anyhow};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::domain::EnvironmentContract;
use crate::terminal::{TerminalDimensions, TerminalEvent, TerminalModel};

pub const CHILD_ENVIRONMENT: &[EnvironmentContract] = &[
    EnvironmentContract {
        key: "TERM",
        value: Some("xterm-256color"),
        requirement: "required",
        missing_behavior: "terminal capabilities would be ambiguous, so the child is not spawned",
    },
    EnvironmentContract {
        key: "COLORTERM",
        value: Some("truecolor"),
        requirement: "optional with an explicit fixed value",
        missing_behavior: "the child renders without true color advertisement",
    },
    EnvironmentContract {
        key: "LANG",
        value: Some("en_US.UTF-8"),
        requirement: "required",
        missing_behavior: "Korean UTF-8 round-trip cannot be claimed, so the child is not spawned",
    },
    EnvironmentContract {
        key: "LC_ALL",
        value: Some("en_US.UTF-8"),
        requirement: "required",
        missing_behavior: "an inherited unsupported macOS locale could corrupt terminal input",
    },
    EnvironmentContract {
        key: "LC_CTYPE",
        value: Some("en_US.UTF-8"),
        requirement: "required",
        missing_behavior: "terminal character classification could differ from the UTF-8 contract",
    },
    EnvironmentContract {
        key: "PROMPT",
        value: Some("herdr-fixture% "),
        requirement: "required for the attached run-owned fixture",
        missing_behavior: "the fixture prompt could expose a local account or hostname",
    },
];

pub struct PtySession {
    master: Option<Box<dyn MasterPty + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    writer: Option<Arc<Mutex<Box<dyn Write + Send>>>>,
    terminal: TerminalModel,
    pane_id: Option<String>,
}

impl PtySession {
    pub fn detached() -> Self {
        Self {
            master: None,
            child: None,
            writer: None,
            terminal: TerminalModel::new(None, Some("Select a terminal pane".to_owned())),
            pane_id: None,
        }
    }

    pub fn attach(
        herdr_bin: &Path,
        session_name: Option<&str>,
        socket_path: &Path,
        pane_id: &str,
    ) -> Result<Self> {
        validate_environment_contract()?;
        let dimensions = TerminalDimensions::default();
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: dimensions.rows as u16,
                cols: dimensions.columns as u16,
                pixel_width: dimensions.columns as u16 * dimensions.cell_width,
                pixel_height: dimensions.rows as u16 * dimensions.cell_height,
            })
            .context("stage=pty.open target=herdr-attach retryable=true")?;
        let mut command = CommandBuilder::new(herdr_bin);
        if let Some(name) = session_name {
            command.arg("--session");
            command.arg(name);
        }
        command.arg("pane");
        command.arg("attach");
        command.arg(pane_id);
        for item in CHILD_ENVIRONMENT {
            if let Some(value) = item.value {
                command.env(item.key, value);
            }
        }
        command.env("HERDR_SOCKET_PATH", socket_path);
        let child = pair.slave.spawn_command(command).with_context(|| {
            format!("stage=pty.spawn target=herdr-pane-attach pane_id={pane_id} retryable=true")
        })?;
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
        let terminal = TerminalModel::new(Some(Arc::clone(&writer)), None);
        let reader_terminal = terminal.clone();
        thread::Builder::new().name("herdr-ide-pty-reader".to_owned()).spawn(move || {
            let mut parser = ansi::Processor::new();
            let mut bytes = [0_u8; 8192];
            loop {
                match reader.read(&mut bytes) {
                    Ok(0) => {
                        let message = reader_terminal.snapshot().ok().map(|view| view.visible_text().to_ascii_lowercase()).filter(|text| text.contains("already attached") || text.contains("attach conflict")).map(|_| "Pane attach conflict: this terminal is already attached elsewhere").unwrap_or("Terminal process exited");
                        reader_terminal.set_failure(message);
                        eprintln!("event=pty.child.exited stage=read-eof status={message:?} retryable=true");
                        break;
                    }
                    Ok(count) => reader_terminal.feed(&mut parser, &bytes[..count]),
                    Err(error) => { reader_terminal.set_failure(&format!("Terminal stream failed: {error}")); eprintln!("event=pty.reader.failed stage=read retryable=true error={error:?}"); break; }
                }
            }
        }).context("stage=pty.reader-thread target=local retryable=false")?;
        Ok(Self {
            master: Some(pair.master),
            child: Some(child),
            writer: Some(writer),
            terminal,
            pane_id: Some(pane_id.to_owned()),
        })
    }

    pub fn terminal(&self) -> TerminalModel {
        self.terminal.clone()
    }
    pub fn process_id(&self) -> Option<u32> {
        self.child.as_ref().and_then(|child| child.process_id())
    }
    pub fn pane_id(&self) -> Option<&str> {
        self.pane_id.as_deref()
    }

    pub fn write_text(&self, text: &str) -> Result<()> {
        let writer = self.writer.as_ref().ok_or_else(|| {
            anyhow!("stage=pty.write target=herdr-pane cause=no-pane-attached retryable=true")
        })?;
        let mut writer = writer
            .lock()
            .map_err(|_| anyhow!("stage=pty.write target=master cause=poisoned-lock"))?;
        writer
            .write_all(text.as_bytes())
            .context("stage=pty.write target=master retryable=true")?;
        writer
            .flush()
            .context("stage=pty.flush target=master retryable=true")
    }

    pub fn drain_terminal_events(&self) -> Vec<TerminalEvent> {
        self.terminal.drain_events()
    }

    pub fn resize(&self, width: u32, height: u32) -> Result<()> {
        let cell_width = 9_u16;
        let cell_height = 18_u16;
        let columns = (width / cell_width as u32).clamp(20, u16::MAX as u32) as u16;
        let rows = (height / cell_height as u32).clamp(5, u16::MAX as u32) as u16;
        self.terminal.resize(TerminalDimensions {
            columns: columns as usize,
            rows: rows as usize,
            cell_width,
            cell_height,
        })?;
        if let Some(master) = self.master.as_ref() {
            master
                .resize(PtySize {
                    rows,
                    cols: columns,
                    pixel_width: width.min(u16::MAX as u32) as u16,
                    pixel_height: height.min(u16::MAX as u32) as u16,
                })
                .context("stage=pty.resize target=master retryable=true")?;
        }
        Ok(())
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut()
            && let Err(error) = child.kill()
        {
            eprintln!(
                "event=pty.cleanup.failed stage=child.kill pane_id={:?} retryable=false error={error:?}",
                self.pane_id
            );
        }
    }
}

fn validate_environment_contract() -> Result<()> {
    for item in CHILD_ENVIRONMENT {
        if item.requirement == "required" && item.value.map(str::is_empty).unwrap_or(true) {
            return Err(anyhow!(
                "stage=pty.environment target={} cause=empty-required-value behavior={}",
                item.key,
                item.missing_behavior
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn environment_contract_is_enumerable_and_required_values_are_nonempty() {
        assert!(CHILD_ENVIRONMENT.len() >= 3);
        assert!(validate_environment_contract().is_ok());
        assert!(CHILD_ENVIRONMENT.iter().all(|item| {
            !item.key.is_empty() && item.value.is_some_and(|value| !value.is_empty())
        }));
    }
}
