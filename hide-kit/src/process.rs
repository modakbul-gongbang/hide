//! The one way the kit runs another program: as an owned child that leads its
//! own tree, with a deadline, and killed with its whole tree when the deadline passes or
//! the caller stops waiting (engineering rule 14; practice `process.md`).
//!
//! Every child the kit starts (`herdr plugin uninstall`, `node --version`,
//! `hcoord daemon ensure`) is short-lived, so none outlives the call that
//! started it, and a raised stop flag ends it at once, so the daemon that owns
//! the kit can quit without waiting out a deadline. Output is capped, because a child that writes forever must not
//! grow the daemon (rule 15).

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use hide_platform::process::OwnedChild;

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
/// that keeps only `PATH` and `HOME`, and waits at most `deadline`, or
/// until `stop` is raised.
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
    stop: &AtomicBool,
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
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    let name = program.display().to_string();
    let mut child = OwnedChild::spawn(&mut command)
        .map_err(|error| format!("{name} could not start: {error}"))?;
    let stdout = child.take_stdout().map(drain);
    let stderr = child.take_stderr().map(drain);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if stop.load(Ordering::Relaxed) => {
                let _ = child.kill_tree();
                let _ = child.wait();
                break Err(format!("{name} was stopped because Hide is quitting"));
            }
            Ok(None) if started.elapsed() >= deadline => {
                let _ = child.kill_tree();
                let _ = child.wait();
                break Err(format!(
                    "{name} did not finish within {} seconds and was stopped",
                    deadline.as_secs()
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill_tree();
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
    let _ = child.kill_tree();
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

    const ROLE: &str = "HIDE_KIT_PROCESS_TEST_ROLE";
    const MARKER: &str = "HIDE_KIT_PROCESS_TEST_MARKER";
    const CHILD_ARGS: &[&str] = &[
        "--exact",
        "process::tests::child_role",
        "--nocapture",
        "--test-threads=1",
    ];

    // A real portable child, as hide-platform's process contract uses. The
    // assertions below are unchanged: a deadline/stop ends the whole tree,
    // and the caller receives output, status and only its explicit env.
    #[test]
    fn child_role() {
        let Ok(role) = std::env::var(ROLE) else {
            return;
        };
        match role.as_str() {
            "tree" => {
                let mut grandchild = Command::new(std::env::current_exe().unwrap())
                    .args(CHILD_ARGS)
                    .env(ROLE, "grandchild")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap();
                thread::sleep(Duration::from_secs(30));
                grandchild.wait().unwrap();
            }
            "grandchild" => {
                thread::sleep(Duration::from_secs(2));
                std::fs::write(std::env::var_os(MARKER).unwrap(), "survived").unwrap();
            }
            "sleep" => thread::sleep(Duration::from_secs(30)),
            "output" => {
                println!(
                    "RESULT {}|{}|{}",
                    std::env::var("HOME").unwrap(),
                    std::env::var("HIDE_KIT_TEST_LEAK").unwrap_or_default(),
                    std::env::var("GIVEN").unwrap()
                );
                eprintln!("oops");
                std::process::exit(3);
            }
            other => panic!("unknown child role {other}"),
        }
    }

    #[test]
    fn a_child_past_its_deadline_is_stopped_with_its_group() {
        let home = tempfile::tempdir().unwrap();
        let marker = home.path().join("grandchild-survived");
        let started = Instant::now();
        let result = run(
            &std::env::current_exe().unwrap(),
            CHILD_ARGS,
            &[
                (ROLE.into(), "tree".into()),
                (MARKER.into(), marker.display().to_string()),
            ],
            home.path(),
            Duration::from_millis(300),
            &AtomicBool::new(false),
        );
        assert!(result.unwrap_err().contains("did not finish"));
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(2500));
        assert!(!marker.exists(), "the grandchild outlived the deadline");
    }

    #[test]
    fn a_raised_stop_ends_the_child_before_its_deadline() {
        let home = tempfile::tempdir().unwrap();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let raiser = {
            let stop = stop.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(200));
                stop.store(true, Ordering::Relaxed);
            })
        };
        let started = Instant::now();
        let result = run(
            &std::env::current_exe().unwrap(),
            CHILD_ARGS,
            &[(ROLE.into(), "sleep".into())],
            home.path(),
            Duration::from_secs(60),
            &stop,
        );
        raiser.join().unwrap();
        assert!(result.unwrap_err().contains("quitting"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn output_and_exit_code_come_back_and_the_environment_is_not_inherited() {
        let home = tempfile::tempdir().unwrap();
        // SAFETY: tests in this module do not read this variable concurrently.
        unsafe { std::env::set_var("HIDE_KIT_TEST_LEAK", "leaked") };
        let finished = run(
            &std::env::current_exe().unwrap(),
            CHILD_ARGS,
            &[
                (ROLE.into(), "output".into()),
                ("GIVEN".into(), "yes".into()),
            ],
            home.path(),
            Duration::from_secs(5),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(finished.code, Some(3));
        let payload = finished
            .stdout
            .lines()
            .find_map(|line| line.split_once("RESULT ").map(|(_, payload)| payload));
        assert_eq!(
            payload,
            Some(format!("{}||yes", home.path().display()).as_str())
        );
        assert_eq!(finished.last_error_line(), "oops");
    }
}
