//! The contract of `hide_platform::ipc`, stated as what a caller observes.
//! The same file runs on macOS, Linux and Windows.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use hide_platform::ipc::{LocalListener, LocalStream};
use tempfile::TempDir;

/// A folder and an endpoint path inside it. Kept short: a Unix socket path
/// is limited to about a hundred bytes.
fn endpoint() -> (TempDir, PathBuf) {
    let folder = tempfile::Builder::new().prefix("hp").tempdir().unwrap();
    let path = folder.path().join("s.sock");
    (folder, path)
}

fn read_line(stream: &mut LocalStream) -> String {
    let mut line = Vec::new();
    let mut byte = [0_u8; 1];
    while stream.read(&mut byte).unwrap() == 1 {
        line.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
    }
    String::from_utf8(line).unwrap()
}

// A second process is how a test kills a peer without killing itself. The
// test binary runs itself with `HIDE_PLATFORM_ROLE` set, and the one test
// below that reads the variable plays the role; without the variable it does
// nothing.
const ROLE: &str = "HIDE_PLATFORM_ROLE";
const ENDPOINT: &str = "HIDE_PLATFORM_ENDPOINT";

#[test]
fn child_role() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let path = PathBuf::from(std::env::var(ENDPOINT).unwrap());
    match role.as_str() {
        "listen" => {
            let _listener = LocalListener::bind(&path).unwrap();
            println!("READY");
            thread::sleep(Duration::from_secs(60));
        }
        "connect" => {
            let _stream = LocalStream::connect(&path).unwrap();
            println!("READY");
            thread::sleep(Duration::from_secs(60));
        }
        other => panic!("unknown role {other}"),
    }
}

struct Spawned {
    child: Child,
}

impl Drop for Spawned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_role(role: &str, path: &Path) -> Spawned {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_role", "--nocapture", "--test-threads=1"])
        .env(ROLE, role)
        .env(ENDPOINT, path)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (ready, heard) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            // libtest prints "test child_role ... " before the line.
            if line.contains("READY") {
                let _ = ready.send(());
            }
        }
    });
    let spawned = Spawned { child };
    heard
        .recv_timeout(Duration::from_secs(30))
        .expect("the child role did not become ready");
    spawned
}

#[test]
fn bytes_travel_both_ways() {
    let (_folder, path) = endpoint();
    let listener = LocalListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let line = read_line(&mut stream);
        stream.write_all(line.to_uppercase().as_bytes()).unwrap();
    });
    let mut client = LocalStream::connect(&path).unwrap();
    client.write_all(b"ping\n").unwrap();
    assert_eq!(read_line(&mut client), "PING\n");
    server.join().unwrap();
}

#[test]
fn a_pair_is_two_connected_ends() {
    let (mut left, mut right) = LocalStream::pair().unwrap();
    left.write_all(b"to the right\n").unwrap();
    right.write_all(b"to the left\n").unwrap();
    assert_eq!(read_line(&mut right), "to the right\n");
    assert_eq!(read_line(&mut left), "to the left\n");
}

#[test]
fn a_closing_peer_ends_the_read_with_zero() {
    let (_folder, path) = endpoint();
    let listener = LocalListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        stream.write_all(b"last\n").unwrap();
    });
    let mut client = LocalStream::connect(&path).unwrap();
    assert_eq!(read_line(&mut client), "last\n");
    server.join().unwrap();
    let mut byte = [0_u8; 1];
    assert_eq!(client.read(&mut byte).unwrap(), 0);
}

#[test]
fn connecting_to_nothing_finds_nothing() {
    let (_folder, path) = endpoint();
    let error = LocalStream::connect(&path).unwrap_err();
    assert!(
        matches!(
            error.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
        ),
        "{error:?}"
    );
}

#[test]
fn a_live_listener_refuses_a_second_bind() {
    let (_folder, path) = endpoint();
    let _first = LocalListener::bind(&path).unwrap();
    let error = LocalListener::bind(&path).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse, "{error:?}");
}

#[test]
fn what_a_killed_listener_left_behind_is_replaced() {
    let (_folder, path) = endpoint();
    let mut holder = spawn_role("listen", &path);
    assert!(path.exists(), "a bound listener has something at its path");
    holder.child.kill().unwrap();
    holder.child.wait().unwrap();

    let listener = LocalListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        stream.write_all(b"new\n").unwrap();
    });
    let mut client = LocalStream::connect(&path).unwrap();
    assert_eq!(read_line(&mut client), "new\n");
    server.join().unwrap();
}

#[test]
fn dropping_the_listener_removes_its_path() {
    let (_folder, path) = endpoint();
    let listener = LocalListener::bind(&path).unwrap();
    assert!(path.exists());
    drop(listener);
    assert!(!path.exists());
}

#[test]
fn a_read_timeout_fires_after_the_time_and_not_much_later() {
    let (_folder, path) = endpoint();
    let listener = LocalListener::bind(&path).unwrap();
    let silent = thread::spawn(move || {
        let stream = listener.accept().unwrap();
        thread::sleep(Duration::from_millis(1500));
        drop(stream);
    });
    let mut client = LocalStream::connect(&path).unwrap();
    client
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    let started = Instant::now();
    let error = client.read(&mut [0_u8; 1]).unwrap_err();
    let waited = started.elapsed();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut, "{error:?}");
    assert!(
        waited >= Duration::from_millis(300),
        "returned early: {waited:?}"
    );
    assert!(waited < Duration::from_secs(2), "returned late: {waited:?}");
    drop(client);
    silent.join().unwrap();
}

#[test]
fn a_timeout_does_not_lose_bytes_that_arrive_later() {
    let (_folder, path) = endpoint();
    let listener = LocalListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        thread::sleep(Duration::from_millis(500));
        stream.write_all(b"late\n").unwrap();
    });
    let mut client = LocalStream::connect(&path).unwrap();
    client
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    assert_eq!(
        client.read(&mut [0_u8; 1]).unwrap_err().kind(),
        std::io::ErrorKind::TimedOut
    );
    client.set_read_timeout(None).unwrap();
    assert_eq!(read_line(&mut client), "late\n");
    server.join().unwrap();
}

#[test]
fn a_timeout_can_be_set_after_the_peer_has_closed_and_its_bytes_still_read() {
    let (_folder, path) = endpoint();
    let listener = LocalListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        stream.write_all(b"ack\nevent\n").unwrap();
    });
    let mut client = LocalStream::connect(&path).unwrap();
    server.join().unwrap();
    thread::sleep(Duration::from_millis(200));
    // macOS refuses `SO_RCVTIMEO` on a socket whose peer has gone.
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    assert_eq!(read_line(&mut client), "ack\n");
    assert_eq!(read_line(&mut client), "event\n");
    assert_eq!(client.read(&mut [0_u8; 1]).unwrap(), 0);
}

#[test]
fn a_zero_timeout_is_refused() {
    let (_folder, path) = endpoint();
    let _listener = LocalListener::bind(&path).unwrap();
    let client = LocalStream::connect(&path).unwrap();
    assert_eq!(
        client
            .set_read_timeout(Some(Duration::ZERO))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
}

#[test]
fn shutdown_from_another_thread_frees_a_blocked_read() {
    let (_folder, path) = endpoint();
    let listener = LocalListener::bind(&path).unwrap();
    let held = thread::spawn(move || {
        let stream = listener.accept().unwrap();
        thread::sleep(Duration::from_secs(10));
        drop(stream);
    });
    let mut client = LocalStream::connect(&path).unwrap();
    let handle = client.shutdown_handle();
    let (done, finished) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut byte = [0_u8; 1];
        let result = client.read(&mut byte);
        let after = client.write(b"x");
        let _ = done.send((result.map_err(|error| error.kind()), after.is_err()));
    });
    // The reader is blocked well before this wakes it.
    thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    handle.shutdown();
    let (read, write_failed) = finished
        .recv_timeout(Duration::from_secs(5))
        .expect("the blocked read was not freed");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(read, Ok(0));
    assert!(write_failed, "a write after shutdown must fail");
    reader.join().unwrap();
    drop(held);
}

#[test]
fn shutdown_frees_a_read_that_has_a_timeout_too() {
    let (_folder, path) = endpoint();
    let listener = LocalListener::bind(&path).unwrap();
    let _held = thread::spawn(move || {
        let stream = listener.accept().unwrap();
        thread::sleep(Duration::from_secs(10));
        drop(stream);
    });
    let mut client = LocalStream::connect(&path).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let handle = client.shutdown_handle();
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        let _ = done.send(client.read(&mut [0_u8; 1]).map_err(|error| error.kind()));
    });
    thread::sleep(Duration::from_millis(300));
    handle.shutdown();
    assert_eq!(
        finished.recv_timeout(Duration::from_secs(5)).unwrap(),
        Ok(0)
    );
}

#[test]
fn the_accepted_stream_names_the_connecting_process() {
    let (_folder, path) = endpoint();
    let listener = LocalListener::bind(&path).unwrap();
    // This process owns the listener; the peer is a different one.
    let (served, accepted) = mpsc::channel();
    let server = thread::spawn(move || {
        let stream = listener.accept().unwrap();
        let _ = served.send(stream.peer_pid());
        thread::sleep(Duration::from_millis(500));
    });
    let peer = spawn_role("connect", &path);
    let pid = accepted
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .expect("this system reports the peer's pid");
    assert_eq!(pid, peer.child.id());
    server.join().unwrap();
}

#[test]
fn the_connecting_side_names_the_listening_process() {
    let (_folder, path) = endpoint();
    let holder = spawn_role("listen", &path);
    let client = LocalStream::connect(&path).unwrap();
    assert_eq!(client.peer_pid().unwrap(), holder.child.id());
}

#[test]
fn the_write_timeout_is_either_kept_or_refused_explicitly() {
    let (_folder, path) = endpoint();
    let _listener = LocalListener::bind(&path).unwrap();
    let client = LocalStream::connect(&path).unwrap();
    let outcome = client.set_write_timeout(Some(Duration::from_secs(1)));
    if cfg!(windows) {
        assert_eq!(outcome.unwrap_err().kind(), std::io::ErrorKind::Unsupported);
    } else {
        outcome.unwrap();
    }
    client.set_write_timeout(None).unwrap();
}

#[cfg(unix)]
#[test]
fn the_endpoint_is_private_to_the_account() {
    use std::os::unix::fs::PermissionsExt;

    let (_folder, path) = endpoint();
    let _listener = LocalListener::bind(&path).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(
        mode & 0o077,
        0,
        "group or other can reach the socket: {mode:o}"
    );
}

#[cfg(windows)]
#[test]
fn the_endpoint_is_private_to_the_account() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
        SE_KERNEL_OBJECT,
    };
    use windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;

    let (_folder, path) = endpoint();
    let _listener = LocalListener::bind(&path).unwrap();
    let client = LocalStream::connect(&path).unwrap();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: the handle is open for the borrow of `client`; every out
    // pointer is either null (not asked for) or a live local.
    let status = unsafe {
        GetSecurityInfo(
            client.as_raw_handle(),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    assert_eq!(status, 0, "GetSecurityInfo failed");
    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: `descriptor` came from GetSecurityInfo above and `text` is a
    // live local the call fills.
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(converted, 0, "the descriptor could not be written as text");
    // SAFETY: the call wrote a NUL-terminated UTF-16 string at `text`.
    let sddl = unsafe {
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        let sddl = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        LocalFree(descriptor);
        sddl
    };
    assert!(sddl.starts_with("D:P"), "the DACL inherits rights: {sddl}");
    for everyone in [";WD)", ";BU)", ";AU)", ";IU)", ";AN)", ";S-1-1-0)"] {
        assert!(!sddl.contains(everyone), "others can connect: {sddl}");
    }
}
