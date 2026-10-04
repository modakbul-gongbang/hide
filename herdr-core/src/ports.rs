//! Which TCP ports the machine is listening on, and where each listener was
//! started from.
//!
//! This reads the system, and nothing more: deciding which pane a listener
//! belongs to is [`attributed_ports`]'s job, and it is pure, so the rule can be
//! tested without a server to point it at.
//!
//! System reads happen on the session-sync coordinator thread, never while
//! the runtime mutex is held and never on a per-event path. Unix uses `lsof`;
//! Windows uses the platform's bounded native listener observation.
//! [`PortsReader::read_if_due`] recomputes only once a refresh window has
//! lapsed.

#[cfg(any(not(windows), test))]
use std::collections::BTreeMap;
use std::path::Path;
#[cfg(any(not(windows), test))]
use std::process::Command;
use std::time::{Duration, Instant};

use crate::model::{ListeningPortSnapshot, ListeningPortsSnapshot, ServerEndpointSnapshot};

/// How stale the port list may be.
///
/// This is the window R12 measures a stopped listener's disappearance against.
/// Wide enough that two `lsof` calls every window are nowhere near a per-tick
/// fork, short enough that starting a dev server shows up while the operator
/// is still looking at the pane.
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

pub struct PortsReader {
    read_at: Option<Instant>,
}

impl PortsReader {
    pub fn new() -> Self {
        Self { read_at: None }
    }

    pub fn read_if_due(&mut self) -> Option<ListeningPortsSnapshot> {
        if self
            .read_at
            .is_some_and(|read_at| read_at.elapsed() < REFRESH_INTERVAL)
        {
            return None;
        }
        let snapshot = read();
        // The refresh interval starts when the sample is complete. A slow
        // system read must not make the very next coordinator tick look due
        // again merely because the read itself took most of the window.
        self.read_at = Some(Instant::now());
        Some(snapshot)
    }
}

impl Default for PortsReader {
    fn default() -> Self {
        Self::new()
    }
}

/// The listeners right now, for a decision that cannot use the last sample.
/// Reads the system, so only a worker thread may call it.
pub(crate) fn read_now() -> ListeningPortsSnapshot {
    read()
}

#[cfg(windows)]
fn read() -> ListeningPortsSnapshot {
    let sockets = match hide_platform::listeners::read() {
        Ok(sockets) => sockets,
        Err(error) => {
            return ListeningPortsSnapshot {
                entries: Vec::new(),
                unavailable_reason: Some(format!(
                    "native TCP listener observation failed: {error}"
                )),
            };
        }
    };
    let mut entries: Vec<_> = sockets
        .into_iter()
        .map(|socket| {
            let host = match socket.address {
                std::net::SocketAddr::V4(address) => {
                    if address.ip().is_unspecified() {
                        "127.0.0.1".to_owned()
                    } else {
                        address.ip().to_string()
                    }
                }
                std::net::SocketAddr::V6(address) => {
                    if address.ip().is_unspecified() {
                        "::1".to_owned()
                    } else if address.scope_id() == 0 {
                        address.ip().to_string()
                    } else {
                        format!("{}%{}", address.ip(), address.scope_id())
                    }
                }
            };
            ListeningPortSnapshot {
                host,
                port: socket.address.port(),
                cwd: socket.cwd.as_str().to_owned(),
            }
        })
        .collect();
    entries.sort_by(|left, right| {
        left.port
            .cmp(&right.port)
            .then_with(|| left.cwd.cmp(&right.cwd))
            .then_with(|| left.host.cmp(&right.host))
    });
    entries.dedup();
    ListeningPortsSnapshot {
        entries,
        unavailable_reason: None,
    }
}

#[cfg(not(windows))]
fn read() -> ListeningPortsSnapshot {
    let listeners = match listening_sockets() {
        Ok(listeners) => listeners,
        Err(reason) => {
            return ListeningPortsSnapshot {
                entries: Vec::new(),
                unavailable_reason: Some(reason),
            };
        }
    };
    if listeners.is_empty() {
        return ListeningPortsSnapshot::default();
    }
    let directories = match working_directories(&listeners.keys().copied().collect::<Vec<_>>()) {
        Ok(directories) => directories,
        Err(reason) => {
            return ListeningPortsSnapshot {
                entries: Vec::new(),
                unavailable_reason: Some(reason),
            };
        }
    };

    let mut entries = Vec::new();
    for (pid, ports) in listeners {
        // A listener whose working directory could not be read belongs to no
        // pane, because the only rule for attributing it is that directory.
        let Some(cwd) = directories.get(&pid) else {
            continue;
        };
        for endpoint in ports {
            entries.push(ListeningPortSnapshot {
                host: endpoint.host,
                port: endpoint.port,
                cwd: cwd.clone(),
            });
        }
    }
    entries.sort_by(|left, right| {
        left.port
            .cmp(&right.port)
            .then_with(|| left.cwd.cmp(&right.cwd))
            .then_with(|| left.host.cmp(&right.host))
    });
    entries.dedup();
    ListeningPortsSnapshot {
        entries,
        unavailable_reason: None,
    }
}

/// Listening TCP sockets, as a pid to ports map.
#[cfg(not(windows))]
fn listening_sockets() -> Result<BTreeMap<u32, Vec<ServerEndpointSnapshot>>, String> {
    let output = run_lsof(&["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pnt"])?;
    Ok(parse_listening_sockets(&output))
}

/// The working directory of each named process.
#[cfg(not(windows))]
fn working_directories(pids: &[u32]) -> Result<BTreeMap<u32, String>, String> {
    if pids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let joined = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let output = run_lsof(&["-a", "-d", "cwd", "-p", &joined, "-F", "pn"])?;
    Ok(parse_working_directories(&output))
}

/// How long one `lsof` may run. A cleanup waits on this read while it holds
/// the daemon's only cleanup lane, so a hung `lsof` must end as an unavailable
/// read, which fails closed, and not as a lane that never frees.
#[cfg(not(windows))]
const LSOF_DEADLINE: Duration = Duration::from_secs(10);

#[cfg(not(windows))]
fn run_lsof(arguments: &[&str]) -> Result<String, String> {
    run_within(Command::new("lsof").args(arguments), LSOF_DEADLINE)
}

#[cfg(any(not(windows), test))]
fn run_within(command: &mut Command, deadline: Duration) -> Result<String, String> {
    let output = hide_host::worktrees::output_within(command, deadline)
        .map_err(|error| format!("lsof could not be run: {error}"))?
        .ok_or_else(|| {
            format!(
                "lsof did not finish within {} seconds and was stopped",
                deadline.as_secs().max(1)
            )
        })?;
    // lsof exits non-zero when some of what it was asked about is gone, which
    // is routine here: a process can exit between the two calls. Whatever it
    // did report is still true, so the output is used and only an empty
    // failure is treated as one.
    if !output.status.success() && output.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!("lsof exited with {}", output.status)
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parses `lsof -F pn` field output.
///
/// The format is one field per line, tagged by its first character, and a `p`
/// line applies to every following line until the next `p`. A socket's `n`
/// field is an address such as `127.0.0.1:5173` or `*:8080`, so the port is
/// what follows the last colon.
#[cfg(any(not(windows), test))]
pub fn parse_listening_sockets(output: &str) -> BTreeMap<u32, Vec<ServerEndpointSnapshot>> {
    let mut sockets: BTreeMap<u32, Vec<ServerEndpointSnapshot>> = BTreeMap::new();
    let mut pid = None;
    let mut family = None;
    for line in output.lines() {
        let Some((tag, value)) = line.split_at_checked(1) else {
            continue;
        };
        match tag {
            "p" => {
                pid = value.trim().parse::<u32>().ok();
                family = None;
            }
            "t" => family = Some(value.trim()),
            "n" => {
                let Some(pid) = pid else { continue };
                // An address with an arrow is a connection, not a listener.
                if value.contains("->") {
                    continue;
                }
                let Some((host, port)) = value.rsplit_once(':') else {
                    continue;
                };
                let Ok(port) = port.trim().parse::<u16>() else {
                    continue;
                };
                if port == 0 {
                    continue;
                }
                let host = host.trim().trim_start_matches('[').trim_end_matches(']');
                let host = match host {
                    "*" if family == Some("IPv6") => "::1".to_owned(),
                    "*" if family == Some("IPv4") => "127.0.0.1".to_owned(),
                    "0.0.0.0" => "127.0.0.1".to_owned(),
                    "::" => "::1".to_owned(),
                    host => match host.parse::<std::net::IpAddr>() {
                        Ok(address) => address.to_string(),
                        Err(_) => continue,
                    },
                };
                let endpoint = ServerEndpointSnapshot { host, port };
                let ports = sockets.entry(pid).or_default();
                if !ports.contains(&endpoint) {
                    ports.push(endpoint);
                }
            }
            _ => {}
        }
    }
    sockets
}

#[cfg(any(not(windows), test))]
pub fn parse_working_directories(output: &str) -> BTreeMap<u32, String> {
    let mut directories = BTreeMap::new();
    let mut pid = None;
    for line in output.lines() {
        let Some((tag, value)) = line.split_at_checked(1) else {
            continue;
        };
        match tag {
            "p" => pid = value.trim().parse::<u32>().ok(),
            "n" => {
                let Some(pid) = pid else { continue };
                let value = value.trim();
                if !value.is_empty() {
                    directories.entry(pid).or_insert_with(|| value.to_owned());
                }
            }
            _ => {}
        }
    }
    directories
}

/// The ports a pane is answerable for: those whose listener was started at or
/// below the pane's own working directory.
///
/// Comparing whole path components is what keeps `/srv/app` from claiming a
/// listener started in `/srv/app-staging`.
pub fn attributed_ports(pane_cwd: &str, listeners: &[ListeningPortSnapshot]) -> Vec<u16> {
    let pane_cwd = pane_cwd.trim();
    if pane_cwd.is_empty() {
        return Vec::new();
    }
    let pane_path = Path::new(pane_cwd);
    let mut ports: Vec<u16> = listeners
        .iter()
        .filter(|listener| Path::new(listener.cwd.as_str()).starts_with(pane_path))
        .map(|listener| listener.port)
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Reuses the same cwd attribution, retaining each observed address.
pub fn attributed_servers(
    pane_cwd: &str,
    listeners: &[ListeningPortSnapshot],
) -> Vec<ServerEndpointSnapshot> {
    if pane_cwd.trim().is_empty() {
        return Vec::new();
    }
    let mut servers: Vec<_> = listeners
        .iter()
        .filter(|listener| Path::new(&listener.cwd).starts_with(Path::new(pane_cwd)))
        .map(|listener| ServerEndpointSnapshot {
            host: listener.host.clone(),
            port: listener.port,
        })
        .collect();
    servers.sort();
    servers.dedup();
    servers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener(port: u16, cwd: &str) -> ListeningPortSnapshot {
        ListeningPortSnapshot {
            host: "127.0.0.1".into(),
            port,
            cwd: cwd.to_owned(),
        }
    }

    #[test]
    fn a_read_that_outlives_its_deadline_is_stopped_and_reported_unavailable() {
        let started = Instant::now();
        let error =
            run_within(Command::new("sleep").arg("30"), Duration::from_millis(200)).unwrap_err();
        assert!(error.contains("did not finish"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(10));
        let ok = run_within(Command::new("echo").arg("p1"), Duration::from_secs(10)).unwrap();
        assert_eq!(ok.trim(), "p1");
    }

    #[test]
    fn field_output_groups_every_port_under_the_process_that_listens_on_it() {
        let output = "p501\ntIPv4\nn*:8080\nn127.0.0.1:5173\np777\nn[::1]:3000\n";
        let sockets = parse_listening_sockets(output);
        assert_eq!(
            sockets.get(&501),
            Some(&vec![
                ServerEndpointSnapshot {
                    host: "127.0.0.1".into(),
                    port: 8080
                },
                ServerEndpointSnapshot {
                    host: "127.0.0.1".into(),
                    port: 5173
                }
            ])
        );
        assert_eq!(
            sockets.get(&777),
            Some(&vec![ServerEndpointSnapshot {
                host: "::1".into(),
                port: 3000
            }])
        );
    }

    #[test]
    fn an_established_connection_is_not_a_listener() {
        let output = "p501\nn127.0.0.1:5173->127.0.0.1:61234\n";
        assert!(parse_listening_sockets(output).is_empty());
    }

    #[test]
    fn a_process_keeps_the_first_working_directory_reported_for_it() {
        let output = "p501\nn/srv/app\np777\nn/srv/other\n";
        let directories = parse_working_directories(output);
        assert_eq!(directories.get(&501).map(String::as_str), Some("/srv/app"));
        assert_eq!(
            directories.get(&777).map(String::as_str),
            Some("/srv/other")
        );
    }

    #[test]
    fn a_pane_claims_listeners_started_at_or_below_its_own_directory() {
        let listeners = [
            listener(5173, "/srv/app"),
            listener(8080, "/srv/app/api"),
            listener(9000, "/srv/other"),
        ];
        assert_eq!(attributed_ports("/srv/app", &listeners), vec![5173, 8080]);
        assert_eq!(attributed_ports("/srv/app/api", &listeners), vec![8080]);
    }

    #[test]
    fn a_sibling_directory_with_a_shared_prefix_is_not_below_the_pane() {
        // `/srv/app-staging` starts with the text `/srv/app` and is not inside
        // it, which is the whole reason attribution compares path components.
        let listeners = [listener(5173, "/srv/app-staging")];
        assert!(attributed_ports("/srv/app", &listeners).is_empty());
    }

    #[test]
    fn a_pane_with_no_working_directory_claims_nothing() {
        assert!(attributed_ports("   ", &[listener(5173, "/srv/app")]).is_empty());
    }

    /// A real native listener checks the platform boundary, not invented field
    /// output that could agree with a wrong parser or foreign-memory layout.
    #[test]
    fn a_real_listener_is_found_and_attributed_to_the_directory_it_runs_in() {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port is bindable");
        let port = listener
            .local_addr()
            .expect("the bound address is readable")
            .port();
        let cwd = std::env::current_dir().expect("the test process has a working directory");

        let read = PortsReader::new()
            .read_if_due()
            .expect("the first read is always due");

        assert_eq!(
            read.unavailable_reason, None,
            "listener observation should be readable here"
        );
        let found = read
            .entries
            .iter()
            .find(|entry| entry.port == port)
            .expect("the listener this test just opened should be reported");
        assert_eq!(Path::new(found.cwd.as_str()), cwd.as_path());
        assert!(
            attributed_ports(&cwd.to_string_lossy(), &read.entries).contains(&port),
            "a listener in the pane's own directory is attributed to it"
        );

        drop(listener);
    }

    #[test]
    fn the_first_read_is_due_and_the_next_one_inside_the_window_is_not() {
        let mut reader = PortsReader::new();
        assert!(reader.read_if_due().is_some());
        assert!(reader.read_if_due().is_none());
    }
}
