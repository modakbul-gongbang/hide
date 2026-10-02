//! The contract of `hide_platform::process`, stated as what a caller observes.
//! The same file runs on macOS, Linux and Windows.
//!
//! A second process is how a test kills a tree without killing itself. The test
//! binary runs itself with `HIDE_PLATFORM_PROC_ROLE` set, and the one test
//! below that reads the variable plays the role; without the variable it does
//! nothing.

use std::io::{BufRead, BufReader, ErrorKind};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use hide_platform::process::{
    OwnedChild, cwd_of, descends_from, is_alive, kill_tree, measure_tree, parent_of, start_time,
    terminate, terminate_group,
};

const ROLE: &str = "HIDE_PLATFORM_PROC_ROLE";

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
fn child_role() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    match role.as_str() {
        "sleep" => {
            println!("READY {}", std::process::id());
            thread::sleep(Duration::from_secs(60));
        }
        // A child with one child of its own that has left no handle behind.
        "tree" => {
            let mut grandchild = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "child_role", "--nocapture", "--test-threads=1"])
                .env(ROLE, "sleep")
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
        other => panic!("unknown role {other}"),
    }
}

fn role_command(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "child_role", "--nocapture", "--test-threads=1"])
        .env(ROLE, role)
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
    child.wait().unwrap();
    assert!(gone_within(pid, Duration::from_secs(5)));
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
