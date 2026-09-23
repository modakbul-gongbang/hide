#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Notify;

fn fake_opener(dir: &Path) -> PathBuf {
    let script = dir.join("fake-opener");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s' \"$$\" > \"$1.pid\"\nsleep 60 &\nprintf '%s' \"$!\" > \"$1.child\"\nwait\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    script
}

fn fake_default_app(dir: &Path) -> PathBuf {
    let script = dir.join("fake-default-app");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s' \"$$\" > \"$1.pid\"\nexec sleep 60\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    script
}

fn wait_for_pid(path: &Path) -> i32 {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(value) = std::fs::read_to_string(path) {
            return value.parse().unwrap();
        }
        assert!(Instant::now() < until, "fake opener never wrote its pid");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}.{}", path.display(), suffix))
}

fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

fn assert_gone(pid: i32) {
    let until = Instant::now() + Duration::from_secs(5);
    while alive(pid) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(pid), "owned fake opener process {pid} survived");
}

#[test]
fn owner_process() {
    let Ok(marker) = std::env::var("HIDED_OWNED_OPENER_TEST_MARKER") else {
        return;
    };
    let marker = PathBuf::from(marker);
    let script = marker.parent().unwrap().join("fake-opener");
    let launched = hided::spawn::spawn_opener(
        Path::new(env!("CARGO_BIN_EXE_hided")),
        script.as_os_str(),
        &marker,
    );
    if std::env::var_os("HIDED_EXPECT_OPENER_TIMEOUT").is_some() {
        assert!(launched.is_err());
        return;
    }
    let _opener = launched.unwrap();
    wait_for_pid(&sidecar(&marker, "pid"));
    std::thread::sleep(Duration::from_secs(30));
}

#[tokio::test]
async fn default_app_handoff_survives_caller_close() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_default_app(dir.path());
    let marker = dir.path().join("handoff");
    hided::spawn::handoff_default_opener(script.as_os_str(), &marker).unwrap();
    let pid = wait_for_pid(&sidecar(&marker, "pid"));
    assert!(alive(pid), "successful default app handoff was closed");
    let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
    // The Tokio process driver reaps the handed-off child; keep the test
    // runtime alive while waiting rather than blocking its only worker.
    let until = Instant::now() + Duration::from_secs(5);
    while alive(pid) && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!alive(pid), "handed-off fake app {pid} survived cleanup");
}

#[test]
fn acceptance_timeout_ends_cli_spawned_before_watcher() {
    let dir = tempfile::tempdir().unwrap();
    fake_opener(dir.path());
    let marker = dir.path().join("acceptance-timeout");
    let owner = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "owner_process", "--nocapture"])
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

#[test]
fn normal_close_reaps_cli_and_its_child() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_opener(dir.path());
    let marker = dir.path().join("normal");
    let mut opener = hided::spawn::spawn_opener(
        Path::new(env!("CARGO_BIN_EXE_hided")),
        script.as_os_str(),
        &marker,
    )
    .unwrap();
    let pid = wait_for_pid(&sidecar(&marker, "pid"));
    let child = wait_for_pid(&sidecar(&marker, "child"));
    opener.stop();
    assert_gone(pid);
    assert_gone(child);
}

#[test]
fn unexpected_supervisor_exit_still_ends_owned_cli_group() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_opener(dir.path());
    let marker = dir.path().join("supervisor-crash");
    let mut opener = hided::spawn::spawn_opener(
        Path::new(env!("CARGO_BIN_EXE_hided")),
        script.as_os_str(),
        &marker,
    )
    .unwrap();
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
    assert!(alive(pid) && alive(child));
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

#[test]
fn sigkill_of_owner_reaps_cli_and_its_child() {
    let dir = tempfile::tempdir().unwrap();
    fake_opener(dir.path());
    let marker = dir.path().join("crash");
    let owner = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "owner_process", "--nocapture"])
        .env("HIDED_OWNED_OPENER_TEST_MARKER", &marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut owner = TestOwner(owner);
    let pid = wait_for_pid(&sidecar(&marker, "pid"));
    let child = wait_for_pid(&sidecar(&marker, "child"));
    assert!(alive(pid) && alive(child));
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
        let mut opener = hided::spawn::spawn_opener(
            Path::new(env!("CARGO_BIN_EXE_hided")),
            script.as_os_str(),
            &marker,
        )
        .unwrap();
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
        PathBuf::from(env!("CARGO_BIN_EXE_hided")),
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
    let script = dir.path().join("quick-opener");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
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
