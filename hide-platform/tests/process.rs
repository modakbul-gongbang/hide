//! The contract of `hide_platform::process`, stated as what a caller observes.
//! The same file runs on macOS, Linux and Windows.
//!
//! A second process is how a test kills a tree without killing itself. The test
//! binary runs itself with `HIDE_PLATFORM_PROC_ROLE` set, and the one test
//! below that reads the variable plays the role; without the variable it does
//! nothing.

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard, mpsc};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use hide_platform::process::GuardedSpawnError;
use hide_platform::process::{
    CaptureFailureKind, MAX_CAPTURE_BYTES, OWNER_LAUNCH_KEYS, OwnedChild, OwnerWatch,
    RUN_OUTPUT_CAP, RunFailure, cwd_of, descendants, descends_from, detach, is_alive, kill_tree,
    measure_tree, parent_of, restrict_to_login_environment, run_to_end, start_time, terminate,
    terminate_group,
};

const ROLE: &str = "HIDE_PLATFORM_PROC_ROLE";

/// The deadline of a test that asks what a launch produced or left behind, not
/// how fast it ran. It is the start wait `ready_number` gives the same child
/// and only ends a child that never answers. A test that is about a deadline
/// (`inherited_output_cannot_extend_capture_deadline`, the uncooperative
/// launch) states its own short one.
const HANG_LIMIT: Duration = Duration::from_secs(30);
// Test-only handoff: arm recovery before the short-lived parent exits.
const PIPE_OWNER: &str = "HIDE_PLATFORM_PROC_PIPE_OWNER";
/// Where the `sleep` role writes its pid once it runs, for a test that cannot
/// read the child's output before the call that started it returns.
const READY_FILE: &str = "HIDE_PLATFORM_PROC_READY_FILE";

/// Windows hands a freed pid to the next process that starts, so a test that
/// asks about a pid it has finished with must not run beside one that starts
/// children. Every test that does either takes this first.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
#[allow(clippy::disallowed_methods)] // a child process the test kills later: it sleeps to stay alive
fn child_role() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let _watch = OwnerWatch::from_launch().unwrap();
    match role.as_str() {
        "proof" => println!("GUARDED {}", _watch.is_some()),
        "echo" => {
            let mut input = Vec::new();
            std::io::stdin().read_to_end(&mut input).unwrap();
            std::io::stdout().write_all(&input).unwrap();
            std::io::stderr().write_all(b"ERROR-MARKER").unwrap();
        }
        "overflow" => {
            std::io::stdout()
                .write_all(&vec![b'x'; MAX_CAPTURE_BYTES + 1])
                .unwrap();
        }
        "held_pipe" => {
            // The parent exits while its own group keeps the stdout pipe
            // open. Capture must time out without joining a blocked reader.
            #[allow(clippy::zombie_processes)]
            let helper = role_command("sleep")
                .stdout(Stdio::inherit())
                .spawn()
                .unwrap();
            println!("READY {}", helper.id());
        }
        "guarded_owner" => {
            let mut child = OwnedChild::spawn_guarded(
                role_command("tree_own_group"),
                Instant::now() + HANG_LIMIT,
            )
            .unwrap();
            let helper = ready_number(child.take_stdout().unwrap());
            println!("OWNED {} {helper}", child.id());
            thread::sleep(Duration::from_secs(60));
        }
        "sleep" => {
            println!("READY {}", std::process::id());
            if let Some(path) = std::env::var_os(READY_FILE) {
                // Renamed into place, so a reader never sees half a number.
                let path = PathBuf::from(path);
                let partial = path.with_extension("partial");
                std::fs::write(&partial, std::process::id().to_string()).unwrap();
                std::fs::rename(&partial, &path).unwrap();
            }
            thread::sleep(Duration::from_secs(60));
        }
        // A child with one child of its own that has left no handle behind.
        "tree" => {
            let mut grandchild = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "child_role", "--nocapture", "--test-threads=1"])
                .env(ROLE, "sleep")
                .env_remove(OWNER_LAUNCH_KEYS[0])
                .env_remove(OWNER_LAUNCH_KEYS[1])
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
            println!("READY {}", grandchild.id());
            let _ = grandchild.wait();
        }
        // A child whose child is in a group of its own: a helper that left
        // with `setsid` or `setpgid` (a job object holds it on Windows).
        "tree_own_group" => {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", "child_role", "--nocapture", "--test-threads=1"])
                .env(ROLE, "sleep")
                .env_remove(OWNER_LAUNCH_KEYS[0])
                .env_remove(OWNER_LAUNCH_KEYS[1])
                .stdout(Stdio::null());
            #[cfg(unix)]
            std::os::unix::process::CommandExt::process_group(&mut command, 0);
            let mut grandchild = command.spawn().unwrap();
            println!("READY {}", grandchild.id());
            let _ = grandchild.wait();
        }
        // Two levels below it: a child that has a `tree` child.
        "deep" => {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "child_role", "--nocapture", "--test-threads=1"])
                .env(ROLE, "tree")
                .env_remove(OWNER_LAUNCH_KEYS[0])
                .env_remove(OWNER_LAUNCH_KEYS[1])
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let stdout = child.stdout.take().unwrap();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line.contains("READY ") {
                    break;
                }
            }
            println!("READY {}", child.id());
            let _ = child.wait();
        }
        // An ordinary supervisor leaves both inherited pipes in its child.
        "exit_with_pipes" | "exit_with_escaped_pipes" => {
            let mut command = Command::new(std::env::current_exe().unwrap());
            #[cfg(unix)]
            if role == "exit_with_escaped_pipes" {
                std::os::unix::process::CommandExt::process_group(&mut command, 0);
            }
            #[allow(clippy::zombie_processes)]
            let helper = command
                .args(["--exact", "child_role", "--nocapture", "--test-threads=1"])
                .env(ROLE, "sleep")
                .spawn()
                .unwrap();
            let path = PathBuf::from(std::env::var_os(PIPE_OWNER).unwrap());
            std::fs::write(
                &path,
                format!("{} {}", helper.id(), start_time(helper.id()).unwrap()),
            )
            .unwrap();
            let started = Instant::now();
            while !path.with_extension("armed").exists() {
                assert!(started.elapsed() < Duration::from_secs(5));
                thread::sleep(Duration::from_millis(10));
            }
            println!("SPOKE out");
            eprintln!("SPOKE err");
            std::process::exit(0);
        }
        // Says something on both outputs and exits with a code of its own.
        "speak" => {
            println!("SPOKE out");
            eprintln!("SPOKE err");
            std::process::exit(3);
        }
        // Writes far past the output cap, then exits.
        "flood" => {
            let line = "x".repeat(1023);
            for _ in 0..512 {
                println!("{line}");
            }
        }
        // A short-lived parent that starts a detached `sleep` and exits, as
        // `hide connect` starts its daemon.
        "detacher" => {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", "child_role", "--nocapture", "--test-threads=1"])
                .env(ROLE, "sleep")
                .env_remove(OWNER_LAUNCH_KEYS[0])
                .env_remove(OWNER_LAUNCH_KEYS[1])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            detach(&mut command).unwrap();
            // Never waited for: the detacher exits at once and leaves its
            // daemon to the system, as `hide connect` does.
            #[allow(clippy::zombie_processes)]
            let daemon = command.spawn().unwrap();
            println!("READY {}", daemon.id());
        }
        // The second process of `a_login_child_gets_the_account_variables_and_nothing_else`:
        // starts a child restricted to the login environment, from an
        // environment that carries variables it must not pass on.
        "login_env_parent" => {
            let mut command = Command::new(std::env::current_exe().unwrap());
            restrict_to_login_environment(&mut command);
            let output = command
                .args(["--exact", "child_role", "--nocapture", "--test-threads=1"])
                .env(ROLE, "login_env")
                .output()
                .unwrap();
            std::io::stdout().write_all(&output.stdout).unwrap();
        }
        "login_env" => {
            for (key, _) in std::env::vars_os() {
                println!("ENVKEY {}", key.to_string_lossy().to_uppercase());
            }
        }
        other => panic!("unknown role {other}"),
    }
}

fn role_command(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "child_role", "--nocapture", "--test-threads=1"])
        .env(ROLE, role)
        .env_remove(OWNER_LAUNCH_KEYS[0])
        .env_remove(OWNER_LAUNCH_KEYS[1])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

/// Reads the number after `READY ` (libtest prints "test child_role ... "
/// before it), waiting for it.
fn ready_number(stdout: std::process::ChildStdout) -> u32 {
    let (ready, heard) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some((_, number)) = line.split_once("READY ") {
                let _ = ready.send(number.trim().parse::<u32>().unwrap());
            }
        }
    });
    heard
        .recv_timeout(Duration::from_secs(30))
        .expect("the child role did not become ready")
}

/// The pid a `sleep` child wrote to `path` once it ran.
#[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
fn announced_pid(path: &std::path::Path) -> u32 {
    let started = Instant::now();
    loop {
        match std::fs::read_to_string(path) {
            Ok(pid) => return pid.parse().unwrap(),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => panic!("cannot read the child's pid: {error}"),
        }
        assert!(started.elapsed() < HANG_LIMIT, "the child never ran");
        thread::sleep(Duration::from_millis(10));
    }
}

#[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
fn gone_within(pid: u32, bound: Duration) -> bool {
    let started = Instant::now();
    while is_alive(pid) {
        if started.elapsed() > bound {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    true
}

/// Failure recovery names only a child this fixture announced while live,
/// and only while the kernel still reports the same process identity.
struct FixtureProcess {
    pid: u32,
    started: u64,
}

impl FixtureProcess {
    fn live(pid: u32) -> Self {
        Self {
            pid,
            started: start_time(pid).unwrap(),
        }
    }
}

impl Drop for FixtureProcess {
    fn drop(&mut self) {
        if start_time(self.pid).ok() == Some(self.started)
            && let Err(source) = kill_tree(self.pid)
        {
            eprintln!(
                "process.fixture_cleanup_failed pid={} error={source}",
                self.pid
            );
        }
    }
}

#[test]
fn guarded_capture_keeps_stdin_payload_and_both_output_streams() {
    let _serial = serial();
    let deadline = Instant::now() + HANG_LIMIT;
    let mut command = role_command("echo");
    command.stdin(Stdio::piped()).stderr(Stdio::piped());
    let mut child = OwnedChild::spawn_guarded(command, deadline).unwrap();
    child
        .take_stdin()
        .unwrap()
        .write_all(b"PAYLOAD-MARKER")
        .unwrap();
    let output = child.capture_until(deadline, 64 * 1024).unwrap();
    assert!(output.status.success());
    assert!(
        output
            .stdout
            .windows(14)
            .any(|part| part == b"PAYLOAD-MARKER")
    );
    assert_eq!(output.stderr, b"ERROR-MARKER");
    assert!(child.try_wait().unwrap().is_some(), "success confirms exit");
}

#[test]
fn only_a_guarded_launch_returns_startup_proof() {
    let _serial = serial();
    let deadline = Instant::now() + HANG_LIMIT;
    let mut guarded = OwnedChild::spawn_guarded(role_command("proof"), deadline).unwrap();
    let output = guarded.capture_until(deadline, 64 * 1024).unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("GUARDED true")
    );
    let deadline = Instant::now() + HANG_LIMIT;
    let mut standalone = OwnedChild::spawn(&mut role_command("proof")).unwrap();
    let output = standalone.capture_until(deadline, 64 * 1024).unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("GUARDED false")
    );
}

#[test]
fn capture_reports_the_callers_byte_limit_without_losing_cleanup() {
    let _serial = serial();
    for limit in [64 * 1024, MAX_CAPTURE_BYTES] {
        let deadline = Instant::now() + HANG_LIMIT;
        let mut child = OwnedChild::spawn_guarded(role_command("overflow"), deadline).unwrap();
        let error = child.capture_until(deadline, limit).unwrap_err();
        assert!(
            matches!(error.kind, CaptureFailureKind::OutputLimit { limit: found } if found == limit)
        );
        assert_eq!(error.stdout.len() + error.stderr.len(), limit);
        assert!(error.cleanup.is_none(), "{error}");
        assert!(child.try_wait().unwrap().is_some());
    }
}

#[test]
fn inherited_output_cannot_extend_capture_deadline() {
    let _serial = serial();
    let started = Instant::now();
    let deadline = started + Duration::from_millis(300);
    let mut child = OwnedChild::spawn(&mut role_command("held_pipe")).unwrap();
    let error = child.capture_until(deadline, 64 * 1024).unwrap_err();
    assert!(matches!(error.kind, CaptureFailureKind::Deadline));
    assert!(
        started.elapsed() < Duration::from_millis(1850),
        "a pipe reader extended the deadline"
    );
    let output = String::from_utf8(error.stdout).unwrap();
    let helper = output
        .lines()
        .find_map(|line| {
            line.split_once("READY ")
                .and_then(|(_, number)| number.trim().parse::<u32>().ok())
        })
        .expect("helper announced its pid");
    if error.cleanup.is_some() {
        // A timeout never asserts an exit it has not observed. The same
        // retained owner can reap after the prompt capture outcome.
        child.kill_tree().unwrap();
        child.wait().unwrap();
    }
    assert!(
        gone_within(helper, Duration::from_secs(5)),
        "owned inherited-pipe helper survived"
    );
}

#[cfg(unix)]
#[test]
fn an_uncooperative_guarded_launch_returns_its_cleanup_state() {
    let _serial = serial();
    let started = Instant::now();
    // An uncooperative Unix executable never acknowledges the watch.
    let mut command = Command::new("/bin/sleep");
    command
        .arg("60")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let error =
        OwnedChild::spawn_guarded(command, started + Duration::from_millis(200)).unwrap_err();
    assert!(started.elapsed() < Duration::from_millis(1850));
    let mut failure = error
        .into_inner()
        .unwrap()
        .downcast::<GuardedSpawnError>()
        .unwrap();
    if let Some(mut child) = failure.child.take() {
        assert!(
            failure.cleanup.is_some(),
            "unconfirmed cleanup retains ownership"
        );
        child.kill_tree().unwrap();
        child.wait().unwrap();
    } else {
        assert!(failure.cleanup.is_none());
    }
}

#[test]
fn abrupt_owner_death_ends_guarded_child_and_helper_outside_its_group() {
    let _serial = serial();
    // A raw owner is intentional: killing only its own Child handle skips
    // all destructors and cannot make its child's cleanup pass by proxy.
    let mut owner = role_command("guarded_owner").spawn().unwrap();
    let stdout = owner.stdout.take().unwrap();
    let (send, recv) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some((_, pair)) = line.split_once("OWNED ") {
                let pids: Vec<u32> = pair
                    .split_whitespace()
                    .map(|pid| pid.parse().unwrap())
                    .collect();
                send.send((pids[0], pids[1])).unwrap();
                return;
            }
        }
    });
    let ready = recv.recv_timeout(Duration::from_secs(5));
    let identities = ready
        .as_ref()
        .ok()
        .map(|(child, helper)| (FixtureProcess::live(*child), FixtureProcess::live(*helper)));
    owner.kill().unwrap();
    owner.wait().unwrap();
    reader.join().unwrap();
    let (child, helper) = ready.expect("guarded owner did not announce its child");
    assert!(
        gone_within(child, Duration::from_secs(5)),
        "guarded child survived owner SIGKILL"
    );
    assert!(
        gone_within(helper, Duration::from_secs(5)),
        "helper outside child group survived"
    );
    drop(identities);
}

#[test]
fn repeated_guarded_work_releases_children_and_capture_resources() {
    let _serial = serial();
    let baseline = measure_tree(std::process::id()).unwrap().descendants;
    for _ in 0..10 {
        let deadline = Instant::now() + HANG_LIMIT;
        let mut command = role_command("echo");
        command.stdin(Stdio::null());
        let mut child = OwnedChild::spawn_guarded(command, deadline).unwrap();
        assert!(
            child
                .capture_until(deadline, 64 * 1024)
                .unwrap()
                .status
                .success()
        );
        drop(child);
        assert_eq!(
            measure_tree(std::process::id()).unwrap().descendants,
            baseline
        );
    }
}

/// A detached child holds none of its parent's standard handles: whoever reads
/// the parent's output sees it end when the parent exits, not when the daemon
/// it started does. Windows hands a child every inheritable handle, and the
/// parent's output pipe is one.
#[test]
fn a_detached_child_leaves_its_parents_output_to_end_with_the_parent() {
    let _serial = serial();
    let mut parent = role_command("detacher").spawn().unwrap();
    let stdout = parent.stdout.take().unwrap();
    let (named, heard_name) = mpsc::channel();
    let (ended, heard_end) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some((_, number)) = line.split_once("READY ") {
                let _ = named.send(number.trim().parse::<u32>().unwrap());
            }
        }
        let _ = ended.send(());
    });
    let daemon = heard_name
        .recv_timeout(Duration::from_secs(30))
        .expect("the detacher named its child");
    assert!(parent.wait().unwrap().success(), "the detacher ended");
    let closed = heard_end.recv_timeout(Duration::from_secs(15));
    let _ = kill_tree(daemon);
    assert!(
        closed.is_ok(),
        "the parent's output stayed open after it exited: its detached child holds it"
    );
}

/// What a supervisor hands a program besides its standard streams: a
/// debugging pipe, a readiness pipe. This test lends one to the detacher the
/// way a supervisor lends it to `hide connect`.
fn lend(command: &mut Command, writer: &std::io::PipeWriter) {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let descriptor = writer.as_raw_fd();
        // SAFETY: only `fcntl` runs after fork, and it is async-signal-safe.
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(descriptor, libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
        // The child inherits every inheritable handle; this one is offered to it alone.
        let _ = command;
        // SAFETY: the handle is the open write end this test owns.
        let marked = unsafe {
            SetHandleInformation(
                writer.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            )
        };
        assert!(
            marked != 0,
            "the write end could not be offered to the child"
        );
    }
}

/// A detached child holds none of the descriptors or handles its parent was
/// given beyond the standard ones. A daemon started by a program a supervisor
/// launched with a pipe of its own would otherwise keep that pipe open for as
/// long as the daemon lives, and a supervisor that waits for the pipe to close
/// (a test runner closing an app, a terminal waiting for its job) waits for
/// the daemon, which is meant to outlive the program that started it.
#[test]
fn a_detached_child_holds_none_of_its_parents_extra_descriptors() {
    let _serial = serial();
    let (mut reader, writer) = std::io::pipe().unwrap();
    let mut command = role_command("detacher");
    lend(&mut command, &writer);
    let mut parent = command.spawn().unwrap();
    drop(writer);
    let stdout = parent.stdout.take().unwrap();
    let (named, heard_name) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some((_, number)) = line.split_once("READY ") {
                let _ = named.send(number.trim().parse::<u32>().unwrap());
            }
        }
    });
    let (ended, heard_end) = mpsc::channel();
    thread::spawn(move || {
        // End of input, or a broken pipe on Windows, is the answer: nothing holds the write end.
        let mut byte = [0u8; 1];
        while matches!(reader.read(&mut byte), Ok(read) if read > 0) {}
        let _ = ended.send(());
    });
    let daemon = heard_name
        .recv_timeout(Duration::from_secs(30))
        .expect("the detacher named its child");
    assert!(parent.wait().unwrap().success(), "the detacher ended");
    let closed = heard_end.recv_timeout(Duration::from_secs(15));
    let _ = kill_tree(daemon);
    assert!(
        closed.is_ok(),
        "the pipe lent to the detacher stayed open after it exited: its detached child holds it"
    );
}

/// An owned `tree` child and the pid of its grandchild.
fn owned_tree() -> (OwnedChild, u32) {
    owned_role("tree")
}

fn owned_role(role: &str) -> (OwnedChild, u32) {
    let mut child = OwnedChild::spawn(&mut role_command(role)).unwrap();
    let number = ready_number(child.take_stdout().unwrap());
    (child, number)
}

#[test]
fn killing_an_owned_tree_ends_the_child_and_what_it_started() {
    let _serial = serial();
    let (mut child, grandchild) = owned_tree();
    let pid = child.id();
    assert!(is_alive(pid) && is_alive(grandchild));
    child.kill_tree().unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success());
    assert!(
        gone_within(grandchild, Duration::from_secs(10)),
        "the grandchild survived"
    );
    assert!(gone_within(pid, Duration::from_secs(10)));
}

#[test]
fn dropping_the_owner_ends_the_tree() {
    let _serial = serial();
    let (child, grandchild) = owned_tree();
    let pid = child.id();
    drop(child);
    assert!(
        gone_within(grandchild, Duration::from_secs(10)),
        "the grandchild survived"
    );
    assert!(gone_within(pid, Duration::from_secs(10)));
}

#[test]
fn ending_a_tree_says_whether_anything_was_running() {
    let _serial = serial();
    let (mut child, grandchild) = owned_tree();
    assert!(
        child.kill_tree().unwrap(),
        "a running tree was there to end"
    );
    child.wait().unwrap();
    assert!(gone_within(grandchild, Duration::from_secs(10)));
    assert!(!child.kill_tree().unwrap(), "nothing was left to end");
}

#[test]
fn killing_by_pid_ends_the_process_and_its_descendants() {
    let _serial = serial();
    let mut command = role_command("tree");
    let mut child = command.spawn().unwrap();
    let grandchild = ready_number(child.stdout.take().unwrap());
    kill_tree(child.id()).unwrap();
    child.wait().unwrap();
    assert!(
        gone_within(grandchild, Duration::from_secs(10)),
        "the grandchild survived"
    );
}

#[test]
fn killing_a_process_that_is_already_gone_is_not_an_error() {
    let _serial = serial();
    let mut child = role_command("sleep").spawn().unwrap();
    let pid = child.id();
    child.kill().unwrap();
    child.wait().unwrap();
    // Windows keeps an exited process until its last handle closes.
    drop(child);
    kill_tree(pid).unwrap();
}

/// What closing a worktree's panes waits for: a running process holds its
/// working folder on Windows, so the folder cannot be renamed until the
/// process has ended, and `start_time` is what says it has.
#[test]
fn a_folder_that_is_a_running_processes_working_folder_moves_once_it_has_ended() {
    let _serial = serial();
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("worktree");
    std::fs::create_dir(&folder).unwrap();
    let mut child = role_command("sleep").current_dir(&folder).spawn().unwrap();
    ready_number(child.stdout.take().unwrap());
    let started = start_time(child.id()).unwrap();
    let aside = root.path().join("aside");
    if cfg!(windows) {
        let refused = std::fs::rename(&folder, &aside).unwrap_err();
        assert!(folder.is_dir() && !aside.exists(), "{refused}");
    }
    let pid = child.id();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        start_time(pid).map_or(true, |now| now != started),
        "an ended process is no longer the one that was read"
    );
    std::fs::rename(&folder, &aside).unwrap();
    assert!(aside.is_dir() && !folder.exists());
}

#[test]
fn a_pid_that_names_no_process_to_signal_is_refused() {
    let _serial = serial();
    for pid in [0, 1, i32::MAX as u32 + 1, u32::MAX] {
        for result in [kill_tree(pid), terminate(pid), terminate_group(pid)] {
            assert_eq!(
                result.unwrap_err().kind(),
                ErrorKind::InvalidInput,
                "pid {pid}"
            );
        }
    }
}

#[test]
fn terminating_ends_the_process() {
    let _serial = serial();
    let mut child = OwnedChild::spawn(&mut role_command("sleep")).unwrap();
    ready_number(child.take_stdout().unwrap());
    terminate(child.id()).unwrap();
    assert!(!child.wait().unwrap().success());
}

#[test]
fn a_process_group_is_ended_where_there_is_one() {
    let _serial = serial();
    let mut child = OwnedChild::spawn(&mut role_command("sleep")).unwrap();
    ready_number(child.take_stdout().unwrap());
    match terminate_group(child.id()) {
        Ok(()) => assert!(!child.wait().unwrap().success()),
        Err(error) => {
            if !cfg!(windows) {
                panic!("only Windows has no group: {error}");
            }
            assert_eq!(error.kind(), ErrorKind::Unsupported);
        }
    }
}

#[test]
fn liveness_is_what_the_system_says() {
    let _serial = serial();
    assert!(is_alive(std::process::id()));
    assert!(!is_alive(0));
    // The init process belongs to another account: it exists.
    #[cfg(unix)]
    assert!(is_alive(1));
    let mut child = role_command("sleep").spawn().unwrap();
    let pid = child.id();
    assert!(is_alive(pid));
    child.kill().unwrap();
    wait_unreaped(&mut child);
    assert!(
        !is_alive(pid),
        "an ended child counted as alive before it was reaped"
    );
    child.wait().unwrap();
}

/// Waits for `child` to end without reaping it, so the system still holds its
/// pid: a Unix zombie that answers signal 0, or a Windows process the
/// `Child`'s handle keeps open.
fn wait_unreaped(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // SAFETY: an all-zero `siginfo_t` is a valid value.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: `waitid` writes only into `info`, and `WNOWAIT` leaves the
        // child to be reaped by `Child::wait`.
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id(),
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        assert_eq!(waited, 0, "{}", std::io::Error::last_os_error());
        // SAFETY: signal 0 only checks that the process exists.
        let signalled = unsafe { libc::kill(child.id() as libc::pid_t, 0) };
        assert_eq!(signalled, 0, "the ended child is no longer the system's");
    }
    #[cfg(windows)]
    child.wait().unwrap();
}

#[test]
fn a_child_names_its_parent_and_starts_after_it() {
    let _serial = serial();
    let mut child = role_command("sleep").spawn().unwrap();
    let pid = child.id();
    ready_number(child.stdout.take().unwrap());
    assert_eq!(parent_of(pid).unwrap(), std::process::id());
    let own = start_time(std::process::id()).unwrap();
    let theirs = start_time(pid).unwrap();
    assert!(
        theirs >= own,
        "the child started before its parent: {theirs} < {own}"
    );
    assert_eq!(
        start_time(pid).unwrap(),
        theirs,
        "a start time does not move"
    );
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn a_pid_that_does_not_exist_is_not_found() {
    let _serial = serial();
    let mut child = role_command("sleep").spawn().unwrap();
    let pid = child.id();
    child.kill().unwrap();
    child.wait().unwrap();
    drop(child);
    assert!(gone_within(pid, Duration::from_secs(5)));
    assert_eq!(parent_of(pid).unwrap_err().kind(), ErrorKind::NotFound);
    assert_eq!(start_time(pid).unwrap_err().kind(), ErrorKind::NotFound);
    assert_eq!(measure_tree(pid).unwrap_err().kind(), ErrorKind::NotFound);
}

#[test]
fn the_working_directory_is_the_kernels_answer_or_unsupported() {
    let _serial = serial();
    let folder = tempfile::tempdir().unwrap();
    let mut child = role_command("sleep")
        .current_dir(folder.path())
        .spawn()
        .unwrap();
    let pid = child.id();
    ready_number(child.stdout.take().unwrap());
    match cwd_of(pid) {
        Ok(path) => {
            if cfg!(windows) {
                panic!("Windows reports no working directory");
            }
            assert_eq!(
                path.canonicalize().unwrap(),
                PathBuf::from(folder.path()).canonicalize().unwrap()
            );
        }
        Err(error) => {
            if !cfg!(windows) {
                panic!("only Windows cannot answer: {error}");
            }
            assert_eq!(error.kind(), ErrorKind::Unsupported);
        }
    }
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn a_tree_is_counted_and_sized_without_forking() {
    let _serial = serial();
    let (child, _grandchild) = owned_tree();
    let measured = measure_tree(child.id()).unwrap();
    assert_eq!(
        measured.descendants, 1,
        "the child's one child, not the child itself"
    );
    assert!(measured.rss_bytes > 0, "resident memory sums over the tree");
    let alone = measure_tree(std::process::id()).unwrap();
    assert!(alone.rss_bytes > 0);
}

#[test]
fn every_level_of_a_nested_tree_is_counted() {
    // 2026-09-20: a units mix-up in the kernel query counted three children
    // as none; the exact count at depth is what guards it.
    let _serial = serial();
    let (child, _middle) = owned_role("deep");
    let measured = measure_tree(child.id()).unwrap();
    assert_eq!(
        measured.descendants, 2,
        "the `tree` child and the child it started"
    );
}

#[test]
fn every_process_under_a_root_is_listed_parents_first_and_the_root_is_not() {
    let _serial = serial();
    let (child, middle) = owned_role("deep");
    let below = descendants(child.id()).unwrap();
    assert_eq!(
        below.len(),
        2,
        "the `tree` child and the child it started: {below:?}"
    );
    assert_eq!(below[0], middle, "the parent before its child: {below:?}");
    assert!(!below.contains(&child.id()));
    assert!(
        descendants(std::process::id())
            .unwrap()
            .contains(&child.id())
    );
    assert!(
        descendants(u32::MAX - 1).unwrap().is_empty(),
        "no process, no children"
    );
}

#[test]
fn a_helper_that_left_the_group_is_still_ended_with_its_owner() {
    let _serial = serial();
    let (mut child, helper) = owned_role("tree_own_group");
    assert!(is_alive(helper));
    child.kill_tree().unwrap();
    child.wait().unwrap();
    assert!(
        gone_within(helper, Duration::from_secs(10)),
        "the helper in its own group survived"
    );
}

#[test]
fn a_tree_descends_from_its_root_and_not_the_other_way() {
    let _serial = serial();
    let (child, grandchild) = owned_tree();
    let (own, pid) = (std::process::id(), child.id());
    assert!(
        descends_from(pid, pid),
        "a process is its own ancestor here"
    );
    assert!(descends_from(pid, own));
    assert!(descends_from(grandchild, pid));
    assert!(descends_from(grandchild, own));
    assert!(!descends_from(own, pid));
    assert!(!descends_from(pid, grandchild));
    assert!(!descends_from(pid, u32::MAX));
}

// A group outlives its leader, and a pid that led one still names it, which is
// how an owner reaches what a dead supervisor left behind.
#[cfg(unix)]
#[test]
#[allow(clippy::disallowed_methods)] // a bounded poll inside the test: it sleeps between observations of a state, bounded by a deadline
fn killing_by_pid_reaches_the_group_of_a_leader_that_already_exited() {
    let _serial = serial();
    let (mut child, grandchild) = owned_tree();
    terminate(child.id()).unwrap();
    // Wait until the leader is gone, so the grandchild is the group's last
    // member and the walk from the leader's pid finds nothing.
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the leader did not end"
        );
        thread::sleep(Duration::from_millis(10));
    }
    kill_tree(child.id()).unwrap();
    assert!(
        gone_within(grandchild, Duration::from_secs(10)),
        "the orphan survived"
    );
    drop(child);
}

fn run_role(
    role: &str,
    deadline: Duration,
    stop: &std::sync::atomic::AtomicBool,
) -> Result<hide_platform::process::Finished, RunFailure> {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "child_role", "--nocapture", "--test-threads=1"])
        .env(ROLE, role);
    run_to_end(&mut command, deadline, stop)
}

#[test]
fn a_child_run_to_its_end_answers_its_code_and_both_outputs() {
    let _serial = serial();
    let finished = run_role(
        "speak",
        Duration::from_secs(30),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(finished.code, Some(3));
    assert!(finished.stdout.contains("SPOKE out"));
    assert!(finished.stderr.contains("SPOKE err"));
}

#[test]
fn a_child_that_writes_past_the_cap_is_read_to_its_end_and_kept_to_the_cap() {
    let _serial = serial();
    let finished = run_role(
        "flood",
        Duration::from_secs(30),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(finished.code, Some(0));
    assert_eq!(finished.stdout.len(), RUN_OUTPUT_CAP);
}

#[test]
fn a_child_past_its_deadline_is_ended_with_its_tree() {
    let _serial = serial();
    let started = Instant::now();
    let failure = run_role(
        "tree",
        Duration::from_millis(500),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(matches!(failure, RunFailure::TimedOut), "{failure:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// The stop is raised once the child runs, so it ends a running child rather
/// than one still starting.
#[test]
fn a_raised_stop_ends_the_child_before_its_deadline() {
    let _serial = serial();
    let folder = tempfile::tempdir().unwrap();
    let ready = folder.path().join("ready");
    let stop = std::sync::atomic::AtomicBool::new(false);
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "child_role", "--nocapture", "--test-threads=1"])
        .env(ROLE, "sleep")
        .env(READY_FILE, &ready);
    let (failure, child, raised) = thread::scope(|scope| {
        let raiser = scope.spawn(|| {
            let child = announced_pid(&ready);
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            (child, Instant::now())
        });
        let failure = run_to_end(&mut command, Duration::from_secs(60), &stop).unwrap_err();
        let (child, raised) = raiser.join().unwrap();
        (failure, child, raised)
    });
    assert!(matches!(failure, RunFailure::Stopped), "{failure:?}");
    assert!(raised.elapsed() < Duration::from_secs(10));
    assert!(!is_alive(child), "the stopped child outlived the call");
}

/// Recovery is armed before the parent exits, so a hanging public call fails
/// within a bound and only its known helper is ended.
#[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
fn inherited_pipe_run(
    role: &str,
    deadline: Duration,
) -> (
    Result<hide_platform::process::Finished, RunFailure>,
    bool,
    Duration,
    bool,
) {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("owner");
    let mut command = role_command(role);
    command.env(PIPE_OWNER, &path);
    let (answered, result) = mpsc::channel();
    let started = Instant::now();
    let runner = thread::spawn(move || {
        let answer = run_to_end(
            &mut command,
            deadline,
            &std::sync::atomic::AtomicBool::new(false),
        );
        answered.send(answer).unwrap();
    });
    let reported = loop {
        match std::fs::read_to_string(&path) {
            Ok(value) => {
                let fields: Vec<_> = value.split_whitespace().collect();
                if fields.len() == 2 {
                    break (
                        fields[0].parse::<u32>().unwrap(),
                        fields[1].parse::<u64>().unwrap(),
                    );
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => panic!("cannot read helper ownership: {error}"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "helper not reported"
        );
        thread::sleep(Duration::from_millis(10));
    };
    struct Recovery(u32, u64);
    impl Drop for Recovery {
        fn drop(&mut self) {
            if start_time(self.0).is_ok_and(|birth| birth == self.1) {
                terminate(self.0).expect("end only the reported helper");
                assert!(
                    gone_within(self.0, Duration::from_secs(5)),
                    "helper recovery failed"
                );
            }
        }
    }
    let recovery = Recovery(reported.0, reported.1);
    assert_eq!(start_time(reported.0).unwrap(), reported.1);
    std::fs::write(path.with_extension("armed"), b"ready").unwrap();
    let answer = result.recv_timeout(Duration::from_secs(5));
    let within_deadline = answer.is_ok();
    let elapsed = started.elapsed();
    let absent_on_return = !is_alive(reported.0);
    drop(recovery);
    let answer = answer.unwrap_or_else(|_| {
        result
            .recv_timeout(Duration::from_secs(5))
            .expect("the recovered pipe reader must finish")
    });
    runner.join().unwrap();
    (answer, within_deadline, elapsed, absent_on_return)
}

/// An exit-0 parent must not wait for its sleeping descendant's pipes.
#[test]
fn normal_exit_ends_an_inherited_pipe_holder_before_draining() {
    let _serial = serial();
    let (answer, within_deadline, elapsed, absent_on_return) =
        inherited_pipe_run("exit_with_pipes", Duration::from_secs(5));
    assert!(
        within_deadline,
        "run_to_end hung after exit 0 while inherited pipes remained open; elapsed {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "the original deadline was exceeded"
    );
    let finished = answer.unwrap();
    assert_eq!(finished.code, Some(0));
    assert!(finished.stdout.contains("SPOKE out"));
    assert!(finished.stderr.contains("SPOKE err"));
    assert!(
        absent_on_return,
        "the inherited-pipe helper outlived the call"
    );
}

/// Unix cannot attribute a helper that escaped and was reparented before its
/// walk, but that helper's inherited pipes must not make a deadline unlimited.
#[cfg(unix)]
#[test]
fn an_escaped_pipe_holder_cannot_extend_the_run_deadline() {
    let _serial = serial();
    let (answer, completed, elapsed, _) =
        inherited_pipe_run("exit_with_escaped_pipes", Duration::from_millis(500));
    assert!(
        completed,
        "pipe draining blocked even after the run deadline"
    );
    assert!(matches!(answer, Err(RunFailure::TimedOut)), "{answer:?}");
    assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
}

/// What a child that must find the account's login is started with: the
/// variables its system reads for the account, the programs and the scratch
/// folders, and none of the hook and nested-session markers the parent was
/// started with. Windows names its own set because Node and the CLI read the
/// home folder from `USERPROFILE` and fail to start without `SystemRoot`.
#[test]
fn a_login_child_gets_the_account_variables_and_nothing_else() {
    let _serial = serial();
    let mut command = role_command("login_env_parent");
    command
        .env("HERDR_ENV", "1")
        .env("CLAUDECODE", "1")
        .env("HIDE_PLATFORM_PROC_UNLISTED", "1");
    let output = command.output().unwrap();
    let keys: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once("ENVKEY ").map(|(_, key)| key.to_owned()))
        .collect();
    assert!(!keys.is_empty(), "the child reported no environment");

    for withheld in ["HERDR_ENV", "CLAUDECODE", "HIDE_PLATFORM_PROC_UNLISTED"] {
        assert!(
            !keys.iter().any(|key| key == withheld),
            "{withheld} {keys:?}"
        );
    }
    let expected: &[&str] = if cfg!(windows) {
        &[
            "SYSTEMROOT",
            "USERPROFILE",
            "TEMP",
            "TMP",
            "PATHEXT",
            "APPDATA",
            "LOCALAPPDATA",
            "PATH",
        ]
    } else {
        &["HOME", "PATH"]
    };
    for needed in expected {
        assert!(keys.iter().any(|key| key == needed), "{needed} {keys:?}");
    }
}
