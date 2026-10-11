//! A pane's hook reaches the daemon that runs its pane when the daemon was
//! started with a moved state folder and the pane's environment lacks it
//! (issue 942): the daemon leaves a record in the default state folder while
//! it runs, `hide workspace memory` finds the daemon's bootstrap through it by
//! the pane's Herdr socket, and the record goes when the daemon ends or,
//! after a crash, when the next daemon of that socket starts.
//!
//! Each daemon is a child process with the test's own home and state folder.
//! It names a Herdr socket that no server listens on: registration needs only
//! the path, which is what Herdr puts in every pane.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(60);

/// Clears what the test's own environment would hand a child of Herdr or
/// Hide, so the child sees only what the test gives it.
fn without_hide(command: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars_os() {
        let upper = key.to_string_lossy().to_uppercase();
        if upper.starts_with("HERDR_") || upper.starts_with("HIDE_") {
            command.env_remove(key);
        }
    }
    command
        .env_remove("XDG_STATE_HOME")
        .env_remove("XDG_CONFIG_HOME")
}

struct Daemon(Child);

impl Daemon {
    /// `idle_secs` lets the daemon end itself once nobody uses it, which is a
    /// graceful stop that needs no signal (a signal sent before the daemon
    /// installs its handler ends it without one); without it, it stays up.
    fn start(home: &Path, state: &Path, socket: &Path, idle_secs: Option<u32>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hided"));
        without_hide(&mut command);
        match idle_secs {
            Some(secs) => command.env("HIDE_IDLE_SECS", secs.to_string()),
            None => command.env("HIDE_KEEP_ALIVE", "1"),
        };
        command
            .env("HOME", home)
            .env("HIDE_STATE_DIR", state)
            .env("HERDR_SOCKET_PATH", socket)
            .env("HIDE_TAILSCALE_BIN", home.join("no-tailscale"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        Self(command.spawn().expect("hided starts"))
    }

    fn kill_hard(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Run {
    _root: tempfile::TempDir,
    home: PathBuf,
    state: PathBuf,
    socket: PathBuf,
    /// A folder outside every checkout the daemon has registered.
    elsewhere: PathBuf,
}

impl Run {
    fn new() -> Self {
        // A Unix socket path is limited to about a hundred bytes.
        let root = tempfile::Builder::new()
            .prefix("ml")
            .tempdir_in("/tmp")
            .unwrap();
        let home = root.path().join("home");
        let elsewhere = root.path().join("elsewhere");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let socket = root.path().join("herdr.sock");
        let state = root.path().join("state");
        Self {
            _root: root,
            home,
            state,
            socket,
            elsewhere,
        }
    }

    fn records(&self) -> PathBuf {
        self.home.join(".hide/state").join("daemons")
    }

    fn wait_until(&self, what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + WAIT;
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            #[allow(clippy::disallowed_methods)] // polls a child process, bounded by the deadline
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn record_count(&self) -> usize {
        std::fs::read_dir(self.records()).map_or(0, |entries| entries.count())
    }

    /// The daemon's bootstrap takes a connection: its record goes out when
    /// the process starts, and its role listens a moment later.
    fn bootstrap_listens(&self) -> bool {
        hided::pane_auth::bootstrap_socket_path(&self.state)
            .is_ok_and(|socket| std::os::unix::net::UnixStream::connect(socket).is_ok())
    }

    /// Why `hide workspace memory` refused, run as a hook in a pane of this
    /// run's Herdr runs it: no state folder, no pane credential, only the
    /// home and the Herdr socket Herdr puts in every pane.
    fn memory_refusal(&self) -> String {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hide"));
        without_hide(&mut command);
        let output = command
            .args(["workspace", "memory", "--event", "SessionStart"])
            .args([
                "--runtime",
                "codex",
                "--session",
                "locator-session",
                "--cwd",
            ])
            .arg(&self.elsewhere)
            .current_dir(&self.elsewhere)
            .env("HOME", &self.home)
            .env("HERDR_SOCKET_PATH", &self.socket)
            .stdin(Stdio::null())
            .output()
            .expect("hide runs");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let answer: serde_json::Value = stdout
            .find('{')
            .and_then(|start| serde_json::from_str(stdout[start..].trim()).ok())
            .unwrap_or_else(|| {
                panic!(
                    "no answer was printed: {stdout} {}",
                    String::from_utf8_lossy(&output.stderr)
                )
            });
        assert_eq!(answer["ok"], false, "{answer}");
        answer["reason"].as_str().unwrap_or_default().to_owned()
    }
}

/// The daemon's own refusal of a caller in no checkout it registered: the
/// bootstrap reached the daemon.
const REACHED: &str = "checkout_not_registered";
/// No daemon listens where the command looked.
const UNREACHED: &str = "hide_unavailable";

#[test]
fn a_pane_of_a_relocated_daemon_reaches_it_and_the_record_goes_with_the_daemon() {
    let run = Run::new();
    let mut daemon = Daemon::start(&run.home, &run.state, &run.socket, Some(8));
    run.wait_until("the daemon's record", || run.record_count() == 1);
    run.wait_until("the daemon's bootstrap", || run.bootstrap_listens());
    assert_eq!(run.memory_refusal(), REACHED);
    // Idle, the daemon ends itself the way `hide stop` ends it.
    run.wait_until("the idle daemon to end", || {
        daemon.0.try_wait().expect("hided is waitable").is_some()
    });
    assert!(daemon.0.wait().unwrap().success());
    assert_eq!(run.record_count(), 0, "a daemon that ends takes its record");
    assert_eq!(run.memory_refusal(), UNREACHED);
}

#[test]
fn a_crashed_daemons_record_is_replaced_by_the_next_daemon_of_that_socket() {
    let run = Run::new();
    let mut crashed = Daemon::start(&run.home, &run.state, &run.socket, None);
    run.wait_until("the first record", || run.record_count() == 1);
    crashed.kill_hard();
    assert_eq!(run.record_count(), 1, "a killed daemon cannot remove it");
    let _next = Daemon::start(&run.home, &run.state, &run.socket, None);
    let record = std::fs::read_dir(run.records())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    run.wait_until("the next daemon's record", || {
        std::fs::read_to_string(&record)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|value| value["pid"] != crashed.0.id())
    });
    assert_eq!(run.record_count(), 1);
    run.wait_until("the next daemon's bootstrap", || run.bootstrap_listens());
    assert_eq!(run.memory_refusal(), REACHED);
}
