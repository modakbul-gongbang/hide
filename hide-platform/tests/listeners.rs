//! Native listener observation is tested against owned, real socket fixtures.
//! Unix callers retain lsof; the native API must explicitly say Unsupported.

#[cfg(not(windows))]
#[test]
fn unix_has_an_explicit_unsupported_native_owner_answer() {
    assert_eq!(
        hide_platform::listeners::read().unwrap_err().kind(),
        std::io::ErrorKind::Unsupported
    );
}

#[cfg(windows)]
mod windows {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    use hide_platform::listeners;
    use hide_platform::process::OwnedChild;

    const ROLE: &str = "HIDE_PLATFORM_LISTENER_FIXTURE";

    #[test]
    fn child_listener() {
        if std::env::var_os(ROLE).is_none() {
            return;
        }
        let ipv4 = TcpListener::bind("127.0.0.1:0").unwrap();
        let ipv6 = TcpListener::bind("[::1]:0").unwrap();
        println!(
            "READY {} {}",
            ipv4.local_addr().unwrap(),
            ipv6.local_addr().unwrap()
        );
        std::io::stdout().flush().unwrap();
        let _ = std::io::stdin().read_exact(&mut [0]);
        drop((ipv4, ipv6));
    }

    struct Fixture {
        child: Option<OwnedChild>,
        output: Option<JoinHandle<()>>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            drop(self.child.take());
            self.output.take().unwrap().join().unwrap();
        }
    }

    #[test]
    fn a_foreign_listener_keeps_its_own_unicode_cwd_and_disappears_when_its_owner_ends() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("native listener 한글");
        std::fs::create_dir(&cwd).unwrap();
        let expected_cwd = hide_platform::fs::identity::canonical(&cwd).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "windows::child_listener",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(ROLE, "1")
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = OwnedChild::spawn(&mut command).unwrap();
        let stdout = child.take_stdout().unwrap();
        let (ready, heard) = mpsc::channel();
        let output = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.unwrap();
                if let Some((_, addresses)) = line.split_once("READY ") {
                    let parsed: Vec<SocketAddr> = addresses
                        .split_whitespace()
                        .map(|address| address.parse().unwrap())
                        .collect();
                    let _ = ready.send(parsed);
                }
            }
        });
        let fixture = Fixture {
            child: Some(child),
            output: Some(output),
        };
        let endpoints = heard
            .recv_timeout(Duration::from_secs(10))
            .expect("owned listener announced both bound addresses");
        assert_eq!(endpoints.len(), 2);
        for endpoint in &endpoints {
            // An actual successful connect establishes that these independently
            // announced addresses are listeners, before asking the reader.
            drop(TcpStream::connect_timeout(endpoint, Duration::from_secs(2)).unwrap());
        }
        let sample = listeners::read().expect("the native observation is complete");
        for endpoint in &endpoints {
            let found = sample
                .iter()
                .find(|listener| {
                    listener.pid == fixture.child.as_ref().unwrap().id()
                        && listener.address == *endpoint
                })
                .expect("the real foreign IPv4/IPv6 listener is reported");
            assert_eq!(found.cwd.as_path(), expected_cwd);
            assert_ne!(found.cwd.as_path(), std::env::current_dir().unwrap());
        }
        drop(fixture);
        let after = listeners::read().expect("the stopped owner's absence is readable");
        assert!(
            after
                .iter()
                .all(|listener| !endpoints.contains(&listener.address))
        );
    }
}
