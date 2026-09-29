//! The one way the kit runs another program: in its own process group, with
//! a deadline, and killed with its whole group when the deadline passes or
//! the caller stops waiting (engineering rule 14; practice `process.md`).
//!
//! Every child the kit starts (`herdr plugin uninstall`, `node --version`,
//! `hcoord daemon ensure`) is short-lived, so none outlives the call that
//! started it. Output is capped, because a child that writes forever must not
//! grow the daemon (rule 15).

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// The most output kept from one stream; the rest is read and dropped.
const OUTPUT_CAP: usize = 64 * 1024;

#[derive(Debug)]
pub struct Finished {
    /// `None` when a signal ended it.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Finished {
    pub fn succeeded(&self) -> bool {
        self.code == Some(0)
    }

    /// The last non-empty line of standard error, for a one-sentence reason.
    pub fn last_error_line(&self) -> String {
        self.stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_owned()
    }
}

/// Runs `program` with `args` and `env` added to an empty-ish environment
/// that keeps only `PATH` and `HOME`, and waits at most `deadline`.
///
/// The environment is built rather than inherited: a daemon launched from a
/// Herdr pane carries that pane's `HERDR_*` identity, and a child that
/// inherited it would act on the operator's Herdr instead of the target's.
pub fn run(
    program: &Path,
    args: &[&str],
    env: &[(String, String)],
    home: &Path,
    deadline: Duration,
) -> Result<Finished, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env(
            "PATH",
            std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    for (key, value) in env {
        command.env(key, value);
    }
    let name = program.display().to_string();
    let mut child = command
        .spawn()
        .map_err(|error| format!("{name} could not start: {error}"))?;
    let pid = child.id() as libc::pid_t;
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() >= deadline => {
                // SAFETY: the group id is the child's own pid, set by
                // `process_group(0)`; signalling it reaches only processes
                // this call started.
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
                let _ = child.wait();
                break Err(format!(
                    "{name} did not finish within {} seconds and was stopped",
                    deadline.as_secs()
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
                let _ = child.wait();
                break Err(format!("{name} could not be waited on: {error}"));
            }
        }
    };
    // The group is gone either way, so the pipes close and the readers end.
    let stdout = stdout
        .map(|reader| reader.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = stderr
        .map(|reader| reader.join().unwrap_or_default())
        .unwrap_or_default();
    let status = status?;
    // A grandchild left in the group after a normal exit is ended too; the
    // kit's children never mean to leave anything behind.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    Ok(Finished {
        code: status.code(),
        stdout,
        stderr,
    })
}

fn drain(mut stream: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let room = OUTPUT_CAP.saturating_sub(kept.len());
                    kept.extend_from_slice(&buffer[..read.min(room)]);
                }
            }
        }
        String::from_utf8_lossy(&kept).into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_past_its_deadline_is_stopped_with_its_group() {
        let home = tempfile::tempdir().unwrap();
        let marker = home.path().join("grandchild-survived");
        let script = format!("(sleep 2; touch '{}') & sleep 30", marker.display());
        let started = Instant::now();
        let result = run(
            Path::new("/bin/sh"),
            &["-c", &script],
            &[],
            home.path(),
            Duration::from_millis(300),
        );
        assert!(result.unwrap_err().contains("did not finish"));
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(2500));
        assert!(!marker.exists(), "the grandchild outlived the deadline");
    }

    #[test]
    fn output_and_exit_code_come_back_and_the_environment_is_not_inherited() {
        let home = tempfile::tempdir().unwrap();
        // SAFETY: tests in this module do not read this variable concurrently.
        unsafe { std::env::set_var("HIDE_KIT_TEST_LEAK", "leaked") };
        let finished = run(
            Path::new("/bin/sh"),
            &["-c", "printf '%s|%s|%s' \"$HOME\" \"$HIDE_KIT_TEST_LEAK\" \"$GIVEN\"; echo oops >&2; exit 3"],
            &[("GIVEN".to_owned(), "yes".to_owned())],
            home.path(),
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(finished.code, Some(3));
        assert_eq!(finished.stdout, format!("{}||yes", home.path().display()));
        assert_eq!(finished.last_error_line(), "oops");
    }
}
