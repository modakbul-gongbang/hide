//! Which TCP ports the machine is listening on, and where each listener was
//! started from.
//!
//! This reads the system, and nothing more: deciding which pane a listener
//! belongs to is [`attributed_ports`]'s job, and it is pure, so the rule can be
//! tested without a server to point it at.
//!
//! Every `lsof` invocation happens here, on the session-sync coordinator
//! thread, never while the runtime mutex is held and never on a per-event path.
//! [`PortsReader::read_if_due`] recomputes only once a refresh window has
//! lapsed.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::model::{ListeningPortSnapshot, ListeningPortsSnapshot};

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
        self.read_at = Some(Instant::now());
        Some(read())
    }
}

impl Default for PortsReader {
    fn default() -> Self {
        Self::new()
    }
}

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
        for port in ports {
            entries.push(ListeningPortSnapshot {
                port,
                cwd: cwd.clone(),
            });
        }
    }
    entries.sort_by(|left, right| {
        left.port
            .cmp(&right.port)
            .then_with(|| left.cwd.cmp(&right.cwd))
    });
    entries.dedup();
    ListeningPortsSnapshot {
        entries,
        unavailable_reason: None,
    }
}

/// Listening TCP sockets, as a pid to ports map.
fn listening_sockets() -> Result<BTreeMap<u32, Vec<u16>>, String> {
    let output = run_lsof(&["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pn"])?;
    Ok(parse_listening_sockets(&output))
}

/// The working directory of each named process.
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

fn run_lsof(arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("lsof")
        .args(arguments)
        .output()
        .map_err(|error| format!("lsof could not be run: {error}"))?;
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
pub fn parse_listening_sockets(output: &str) -> BTreeMap<u32, Vec<u16>> {
    let mut sockets: BTreeMap<u32, Vec<u16>> = BTreeMap::new();
    let mut pid = None;
    for line in output.lines() {
        let Some((tag, value)) = line.split_at_checked(1) else {
            continue;
        };
        match tag {
            "p" => pid = value.trim().parse::<u32>().ok(),
            "n" => {
                let Some(pid) = pid else { continue };
                // An address with an arrow is a connection, not a listener.
                if value.contains("->") {
                    continue;
                }
                let Some(port) = value
                    .rsplit(':')
                    .next()
                    .and_then(|port| port.trim().parse::<u16>().ok())
                else {
                    continue;
                };
                let ports = sockets.entry(pid).or_default();
                if !ports.contains(&port) {
                    ports.push(port);
                }
            }
            _ => {}
        }
    }
    sockets
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn listener(port: u16, cwd: &str) -> ListeningPortSnapshot {
        ListeningPortSnapshot {
            port,
            cwd: cwd.to_owned(),
        }
    }

    #[test]
    fn field_output_groups_every_port_under_the_process_that_listens_on_it() {
        let output = "p501\nn*:8080\nn127.0.0.1:5173\np777\nn[::1]:3000\n";
        let sockets = parse_listening_sockets(output);
        assert_eq!(sockets.get(&501), Some(&vec![8080, 5173]));
        assert_eq!(sockets.get(&777), Some(&vec![3000]));
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

    /// The parse is checked against this platform's real `lsof`, because the
    /// field format is the assumption most likely to be wrong and a unit test
    /// over invented output would agree with itself.
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
            "lsof should be readable here"
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
