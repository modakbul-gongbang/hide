//! An explicit `HIDE_OPEN_COMMAND` helper is owned on every system: stopping
//! it, its timeout, the daemon's shutdown and the daemon's death each end the
//! helper and the child it started. The fake helper is this test binary run
//! again through a two-line script, so it does the same thing on macOS, Linux
//! and Windows; only the script's language differs.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hide_platform::process::{is_alive, start_time};
use tokio::sync::Notify;

const ROLE: &str = "HIDED_FAKE_OPENER_ROLE";
const MARKER: &str = "HIDED_FAKE_OPENER_MARKER";

/// A script in `dir` that runs [`fake_program`] in `role` with the file it is
/// given as the marker: what an operator's `HIDE_OPEN_COMMAND` is to hided.
fn fake(dir: &Path, name: &str, role: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let entry = "--ignored --exact fake_program --nocapture";
    #[cfg(unix)]
    let script = {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join(name);
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n{MARKER}=\"$1\" {ROLE}={role} exec '{}' {entry}\n",
                exe.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        script
    };
    #[cfg(windows)]
    let script = {
        let script = dir.join(format!("{name}.cmd"));
        std::fs::write(
            &script,
            format!(
                "@set \"{MARKER}=%~1\"\r\n@set \"{ROLE}={role}\"\r\n@\"{}\" {entry}\r\n",
                exe.display()
            ),
        )
        .unwrap();
        script
    };
    script
}

fn fake_opener(dir: &Path) -> PathBuf {
    fake(dir, "fake-opener", "opener")
}

/// The fake helper. `opener` writes its pid beside the marker, starts a child
/// that writes its own (`child`) and waits for it, the way a CLI that starts
/// an app and stays attached would; `app` writes its pid and stays; `quick`
/// returns at once. Ignored, so a plain `cargo test` never counts it as a
/// passing test of anything; it is an entry point.
#[test]
#[ignore = "subprocess entry point; the fake helper script runs it with --ignored --exact"]
#[allow(clippy::disallowed_methods)] // a child process the test kills later: it sleeps to stay alive
fn fake_program() {
    let marker = PathBuf::from(std::env::var_os(MARKER).expect("run by a fake helper script"));
    let write_pid =
        |suffix| std::fs::write(sidecar(&marker, suffix), std::process::id().to_string()).unwrap();
    match std::env::var(ROLE).unwrap().as_str() {
        "opener" => {
            // The path exactly as the helper was handed it.
            std::fs::write(sidecar(&marker, "arg"), marker.to_string_lossy().as_bytes()).unwrap();
            write_pid("pid");
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "fake_program", "--nocapture"])
                .env(ROLE, "child")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let _ = child.wait();
        }
        "child" => {
            write_pid("child");
            std::thread::sleep(Duration::from_secs(60));
        }
        "app" => {
            write_pid("pid");
            std::thread::sleep(Duration::from_secs(60));
        }
        "quick" => {}
        other => panic!("unknown fake helper role {other}"),
    }
}

/// The hided binary the Unix supervisor runs as, launched once before any
/// test times it. macOS holds the first exec of a freshly linked binary for
/// about two seconds while it assesses it (measured 2.08-2.14 s, no user or
/// system time), and `cargo build` and `cargo test` link different hided
/// binaries, so a test run usually starts with a new one. Without this, that
/// one-time hold falls inside the supervisor's two-second acceptance window
/// and a launch fails as if the supervisor could not start. The daemon never
/// pays it: its supervisor is the binary it is already running. Windows runs
/// no supervisor, and there the binary would start a daemon.
fn supervisor() -> &'static Path {
    let path = Path::new(env!("CARGO_BIN_EXE_hided"));
    #[cfg(unix)]
    {
        static WARM: std::sync::Once = std::sync::Once::new();
        WARM.call_once(|| {
            let _ = Command::new(path)
                .arg("--open-helper")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        });
    }
    path
}

/// A process the fake helper started, named by its pid and its start time so
/// a later process that reuses the pid (Windows hands a freed pid to the next
/// process that starts) is not mistaken for it.
#[derive(Clone, Copy, Debug)]
struct Started {
    pid: u32,
    at: u64,
}

impl Started {
    fn alive(self) -> bool {
        is_alive(self.pid) && start_time(self.pid).is_ok_and(|at| at == self.at)
    }
}

#[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
fn wait_for_pid(path: &Path) -> Started {
    // A test binary's first start on a Windows runner can take seconds.
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        // The fake helper creates the file before it writes into it, so an
        // empty read means the pid is not there yet.
        if let Ok(value) = std::fs::read_to_string(path)
            && let Ok(pid) = value.parse()
        {
            // A helper that is already gone has no start; 0 is no start a
            // live process has, so it reads as gone.
            let at = start_time(pid).unwrap_or(0);
            return Started { pid, at };
        }
        assert!(Instant::now() < until, "fake helper never wrote its pid");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}.{}", path.display(), suffix))
}

#[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
fn assert_gone(process: Started) {
    let until = Instant::now() + Duration::from_secs(5);
    while process.alive() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!process.alive(), "owned fake helper {process:?} survived");
}

/// The owner process the tests below spawn: this test binary run again with
/// `--ignored --exact owner_process`. Ignored, so a plain `cargo test` never
/// counts it as a passing test of anything; it is an entry point.
#[test]
#[ignore = "subprocess entry point; the owner tests run it with --ignored --exact"]
#[allow(clippy::disallowed_methods)] // a child process the test kills later: it sleeps to stay alive
fn owner_process() {
    let marker = PathBuf::from(
        std::env::var_os("HIDED_OWNED_OPENER_TEST_MARKER")
            .expect("owner_process is spawned by a test, with its marker"),
    );
    let script = fake_opener(marker.parent().unwrap());
    let launched = hided::spawn::spawn_opener(supervisor(), script.as_os_str(), &marker);
    if std::env::var_os("HIDED_EXPECT_OPENER_TIMEOUT").is_some() {
        assert!(launched.is_err());
        return;
    }
    let _opener = launched.unwrap();
    wait_for_pid(&sidecar(&marker, "pid"));
    std::thread::sleep(Duration::from_secs(30));
}

/// Windows hands the default to `ShellExecuteW`, which starts no child.
#[cfg(unix)]
#[tokio::test]
async fn default_app_handoff_survives_caller_close() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake(dir.path(), "fake-default-app", "app");
    let marker = dir.path().join("handoff");
    hided::spawn::handoff_default_opener(script.as_os_str(), &marker).unwrap();
    let app = wait_for_pid(&sidecar(&marker, "pid"));
    assert!(app.alive(), "successful default app handoff was closed");
    let _ = unsafe { libc::kill(app.pid as i32, libc::SIGKILL) };
    // The Tokio process driver reaps the handed-off child; keep the test
    // runtime alive while waiting rather than blocking its only worker.
    let until = Instant::now() + Duration::from_secs(5);
    while app.alive() && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!app.alive(), "handed-off fake app {app:?} survived cleanup");
}

/// The supervisor's acceptance handshake exists only on Unix.
#[cfg(unix)]
#[test]
fn acceptance_timeout_ends_cli_spawned_before_watcher() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("acceptance-timeout");
    supervisor();
    let owner = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "owner_process", "--nocapture"])
        .env("HIDED_OWNED_OPENER_TEST_MARKER", &marker)
        .env("HIDED_EXPECT_OPENER_TIMEOUT", "1")
        .env("HIDE_OPEN_HELPER_TEST_PAUSE_MS", "3000")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut owner = TestOwner(owner);
    let pid = wait_for_pid(&sidecar(&marker, "pid"));
    let child = wait_for_pid(&sidecar(&marker, "child"));
    assert!(owner.0.wait().unwrap().success());
    assert_gone(pid);
    assert_gone(child);
}

/// A canonical path is `\\?\C:\...` on Windows, which `cmd.exe` cannot read;
/// the helper is handed the plain spelling of the same file.
#[tokio::test]
async fn a_helper_is_handed_the_plain_spelling_of_a_canonical_path() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_opener(dir.path());
    let handler = handler(&script, Arc::new(Notify::new()));
    let canonical = std::fs::canonicalize(dir.path()).unwrap().join("canonical");
    let plain = hide_platform::fs::identity::canonical(dir.path())
        .unwrap()
        .join("canonical");
    assert_eq!(handler.launch(&canonical), Ok(()));
    let handed = sidecar(&plain, "arg");
    let until = Instant::now() + Duration::from_secs(20);
    while std::fs::read_to_string(&handed).map_or(true, |text| text.is_empty()) {
        assert!(
            Instant::now() < until,
            "fake helper never wrote its argument"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        std::fs::read_to_string(&handed).unwrap(),
        plain.to_string_lossy()
    );
    // The runtime's end drops the handler's task, whose owner ends the helper.
}

#[test]
fn normal_close_reaps_cli_and_its_child() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_opener(dir.path());
    let marker = dir.path().join("normal");
    let mut opener = hided::spawn::spawn_opener(supervisor(), script.as_os_str(), &marker).unwrap();
    let pid = wait_for_pid(&sidecar(&marker, "pid"));
    let child = wait_for_pid(&sidecar(&marker, "child"));
    opener.stop();
    assert_gone(pid);
    assert_gone(child);
}

#[cfg(unix)]
#[test]
#[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
fn unexpected_supervisor_exit_still_ends_owned_cli_group() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_opener(dir.path());
    let marker = dir.path().join("supervisor-crash");
    let mut opener = hided::spawn::spawn_opener(supervisor(), script.as_os_str(), &marker).unwrap();
    let pid = wait_for_pid(&sidecar(&marker, "pid"));
    let child = wait_for_pid(&sidecar(&marker, "child"));
    assert_eq!(
        unsafe { libc::kill(opener.supervisor_pid() as i32, libc::SIGKILL) },
        0
    );
    let until = Instant::now() + Duration::from_secs(5);
    while !opener.try_wait().unwrap() {
        assert!(Instant::now() < until, "supervisor did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(pid.alive() && child.alive());
    opener.stop();
    assert_gone(pid);
    assert_gone(child);
}

struct TestOwner(Child);

impl Drop for TestOwner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The owner dies without running a destructor (`SIGKILL`, `TerminateProcess`).
#[test]
fn killing_the_owner_reaps_cli_and_its_child() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("crash");
    supervisor();
    let owner = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "owner_process", "--nocapture"])
        .env("HIDED_OWNED_OPENER_TEST_MARKER", &marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut owner = TestOwner(owner);
    let pid = wait_for_pid(&sidecar(&marker, "pid"));
    let child = wait_for_pid(&sidecar(&marker, "child"));
    assert!(pid.alive() && child.alive());
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    assert_gone(pid);
    assert_gone(child);
}

#[test]
fn repeated_owned_helpers_are_reaped_between_requests() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_opener(dir.path());
    for index in 0..16 {
        let marker = dir.path().join(format!("request-{index}"));
        let mut opener =
            hided::spawn::spawn_opener(supervisor(), script.as_os_str(), &marker).unwrap();
        let pid = wait_for_pid(&sidecar(&marker, "pid"));
        let child = wait_for_pid(&sidecar(&marker, "child"));
        opener.stop();
        assert_gone(pid);
        assert_gone(child);
    }
}

fn handler(script: &Path, shutdown: Arc<Notify>) -> hided::opener::OpenHandler {
    hided::opener::OpenHandler::new(
        Some(script.to_path_buf()),
        shutdown,
        supervisor().to_path_buf(),
    )
}

async fn wait_until_idle(handler: &hided::opener::OpenHandler) {
    let until = Instant::now() + Duration::from_secs(5);
    while handler.in_flight() != 0 && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(handler.in_flight(), 0, "owned helper slot did not release");
}

#[tokio::test]
async fn launch_cap_and_shutdown_reap_owned_children() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_opener(dir.path());
    let shutdown = Arc::new(Notify::new());
    let handler = handler(&script, Arc::clone(&shutdown));
    let mut children = Vec::new();
    for index in 0..4 {
        let marker = dir.path().join(format!("open-{index}"));
        assert_eq!(handler.launch(&marker), Ok(()));
        children.push(wait_for_pid(&sidecar(&marker, "pid")));
        children.push(wait_for_pid(&sidecar(&marker, "child")));
    }
    assert_eq!(handler.in_flight(), 4);
    assert_eq!(
        handler.launch(&dir.path().join("fifth")),
        Err("over_budget")
    );
    tokio::task::yield_now().await;
    shutdown.notify_waiters();
    wait_until_idle(&handler).await;
    for pid in children {
        assert_gone(pid);
    }
}

#[tokio::test]
async fn owned_cli_launch_times_out_and_reaps_its_child() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_opener(dir.path());
    let handler = handler(&script, Arc::new(Notify::new()));
    let marker = dir.path().join("timeout");
    assert_eq!(handler.launch(&marker), Ok(()));
    let pid = wait_for_pid(&sidecar(&marker, "pid"));
    let child = wait_for_pid(&sidecar(&marker, "child"));
    let until = Instant::now() + Duration::from_secs(13);
    while handler.in_flight() != 0 && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        handler.in_flight(),
        0,
        "ten-second timeout did not release slot"
    );
    assert_gone(pid);
    assert_gone(child);
}

#[tokio::test]
async fn thirteenth_quick_launch_is_over_budget() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake(dir.path(), "quick-opener", "quick");
    let handler = handler(&script, Arc::new(Notify::new()));
    for index in 0..12 {
        assert_eq!(
            handler.launch(&dir.path().join(format!("quick-{index}"))),
            Ok(())
        );
        wait_until_idle(&handler).await;
    }
    assert_eq!(
        handler.launch(&dir.path().join("thirteenth")),
        Err("over_budget")
    );
}
