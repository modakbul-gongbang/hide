//! A pane's hook finds the Memory store of a daemon started with a moved state
//! folder (issue 942): the daemon leaves a record in the default state folder
//! while it runs, the hook resolves its pane's Herdr socket through it, and the
//! record goes when the daemon ends or, after a crash, when the next daemon of
//! that socket starts.
//!
//! Each daemon is a child process with the test's own home and state folder.
//! It names a Herdr socket that no server listens on: registration needs only
//! the path, which is what Herdr puts in every pane.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(60);

struct Daemon(Child);

impl Daemon {
    /// `idle_secs` lets the daemon end itself once nobody uses it, which is a
    /// graceful stop that needs no signal (a signal sent before the daemon
    /// installs its handler ends it without one); without it, it stays up.
    fn start(home: &Path, state: &Path, socket: &Path, idle_secs: Option<u32>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hided"));
        for (key, _) in std::env::vars_os() {
            let upper = key.to_string_lossy().to_uppercase();
            if upper.starts_with("HERDR_") || upper.starts_with("HIDE_") {
                command.env_remove(key);
            }
        }
        match idle_secs {
            Some(secs) => command.env("HIDE_IDLE_SECS", secs.to_string()),
            None => command.env("HIDE_KEEP_ALIVE", "1"),
        };
        command
            .env("HOME", home)
            .env_remove("XDG_STATE_HOME")
            .env_remove("XDG_CONFIG_HOME")
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
}

impl Run {
    fn new() -> Self {
        // A Unix socket path is limited to about a hundred bytes.
        let root = tempfile::Builder::new()
            .prefix("ml")
            .tempdir_in("/tmp")
            .unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let socket = root.path().join("herdr.sock");
        let state = root.path().join("state");
        Self {
            _root: root,
            home,
            state,
            socket,
        }
    }

    fn records(&self) -> PathBuf {
        self.home.join(".hide/state").join("daemons")
    }

    fn store_a_pane_reads(&self) -> PathBuf {
        hide_agent_hooks::memory::database_path_in(&self.home, self.socket.to_str())
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
}

#[test]
fn a_pane_of_a_relocated_daemon_reads_its_store_and_the_record_goes_with_the_daemon() {
    let run = Run::new();
    let mut daemon = Daemon::start(&run.home, &run.state, &run.socket, Some(8));
    let default_store = run.home.join(".hide/state/project-memory.sqlite3");
    run.wait_until("the daemon's record", || {
        run.store_a_pane_reads() != default_store
    });
    assert_eq!(
        run.store_a_pane_reads(),
        run.state.join("project-memory.sqlite3")
    );
    // Idle, the daemon ends itself the way `hide stop` ends it.
    run.wait_until("the idle daemon to end", || {
        daemon.0.try_wait().expect("hided is waitable").is_some()
    });
    assert!(daemon.0.wait().unwrap().success());
    assert_eq!(run.record_count(), 0, "a daemon that ends takes its record");
    assert_eq!(run.store_a_pane_reads(), default_store);
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
    assert_eq!(
        run.store_a_pane_reads(),
        run.state.join("project-memory.sqlite3")
    );
}
