//! The node contract (PRD core-host-node b5): the core's own node
//! (`hide_node::Local`) and a device's node reached over SSH, `hided node
//! serve` behind the loopback SSH server, answer the same calls alike, and a
//! device's channels share its one connection.
#![cfg(unix)]

#[path = "support/ssh_server.rs"]
mod ssh_server;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use hide_node_link::device::{
    DeviceConnector, DeviceTransport, HOST_CONSENT_CONTRACT, HostConsent,
};
use hide_node_link::panes::NodeEvent;
use hide_node_link::protocol::{Call, RootOpened, RootRef};
use hide_node_link::{NodeLink, call_as};
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(20);
const ALIAS: &str = "contract-device";

/// The device's private account: its home and the environment every process
/// the SSH server starts for it gets, and nothing of this process's.
struct Account {
    home: PathBuf,
    values: Vec<(OsString, OsString)>,
}

impl ssh_server::Account for Account {
    fn command(&self, program: &OsStr) -> Command {
        let mut command = Command::new(program);
        command
            .env_clear()
            .envs(self.values.iter().map(|(name, value)| (name, value)))
            .current_dir(&self.home);
        command
    }

    fn home(&self) -> &Path {
        &self.home
    }
}

/// A device on the loopback SSH server with a committed checkout and a
/// change in it, and its node started over one connection.
struct Device {
    _root: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    ssh: ssh_server::Ssh,
    transport: Arc<dyn DeviceTransport>,
    link: Arc<dyn NodeLink>,
    /// The device's state folder, where its node binds its pane socket.
    state: PathBuf,
    /// What the link's pane events told this process.
    events: Arc<Events>,
}

/// The pane events of the device's link, as hided would receive them.
#[derive(Default)]
struct Events {
    closed: std::sync::Mutex<Vec<String>>,
}

impl hide_node::ssh::PaneEvents for Events {
    fn event(&self, _node: &str, _link: &hide_node::ssh::RemoteHost, _event: NodeEvent) {}

    fn closed(&self, node: &str, _link: &hide_node::ssh::RemoteHost) {
        self.closed.lock().unwrap().push(node.to_owned());
    }
}

fn run(command: &mut Command) {
    let output = command.output().unwrap();
    assert!(output.status.success(), "{command:?}: {output:?}");
}

impl Device {
    fn start() -> Self {
        // Socket paths below this folder must fit a Unix socket address.
        let root = tempfile::Builder::new()
            .prefix("nc")
            .tempdir_in("/tmp")
            .unwrap();
        // The canonical path: the install refuses a link among a state
        // folder's parents, and `/tmp` is one on macOS.
        let base = fs::canonicalize(root.path()).unwrap();
        let home = base.join("h");
        let project = home.join("project");
        fs::create_dir_all(&project).unwrap();
        let herdr_socket = base.join("s.sock");
        let state = base.join("st");
        let account = Account {
            home: home.clone(),
            values: vec![
                ("HOME".into(), home.clone().into()),
                ("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
                ("SHELL".into(), "/bin/sh".into()),
                ("HIDE_STATE_DIR".into(), state.clone().into()),
                ("HIDE_TEST_HIDED".into(), env!("CARGO_BIN_EXE_hided").into()),
            ],
        };
        let git = |args: &[&str]| {
            let mut command = ssh_server::Account::command(&account, OsStr::new("/usr/bin/git"));
            command.args(["-c", "init.defaultBranch=main", "-c", "user.name=Fixture"]);
            command.args(["-c", "user.email=fixture@example.invalid", "-C"]);
            command.arg(&project).args(args);
            run(&mut command);
        };
        fs::write(project.join("a.txt"), "one\n").unwrap();
        git(&["init", "-q"]);
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "first"]);
        fs::write(project.join("a.txt"), "two\n").unwrap();
        fs::write(project.join("b.txt"), "new\n").unwrap();
        // The device's Herdr answers where its server listens; nothing is
        // served there but the socket a Herdr channel opens.
        let bin = home.join(".local/bin");
        fs::create_dir_all(&bin).unwrap();
        let status = json!({"running": true, "socket": herdr_socket, "version": "0.9.1"});
        executable(
            &bin.join("herdr"),
            &format!("#!/bin/sh\nprintf '%s\\n' '{status}'\n"),
        );
        // A Herdr that hangs up at once: a channel to it opens, and asking
        // it about a pane fails rather than waits.
        let herdr = UnixListener::bind(&herdr_socket).unwrap();
        std::thread::spawn(move || for _ in herdr.incoming() {});

        let keys = base.join("k");
        fs::create_dir(&keys).unwrap();
        for name in ["host", "client"] {
            run(Command::new("/usr/bin/ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(keys.join(name)));
        }
        let ssh = ssh_server::Ssh::start(
            account,
            herdr_socket,
            &keys.join("host"),
            &keys.join("client"),
        )
        .unwrap();
        let local = base.join("l");
        fs::create_dir_all(local.join(".ssh")).unwrap();
        let public = fs::read_to_string(keys.join("host.pub")).unwrap();
        fs::write(
            local.join(".ssh/known_hosts"),
            format!("[127.0.0.1]:{} {}\n", ssh.port, public.trim()),
        )
        .unwrap();
        fs::write(
            local.join(".ssh/config"),
            format!(
                "Host {ALIAS}\n  HostName 127.0.0.1\n  Port {}\n  User fixture\n  IdentityFile {}\n  IdentityAgent none\n",
                ssh.port,
                keys.join("client").display()
            ),
        )
        .unwrap();
        // The build this daemon carries for the device: a launcher for the
        // `hided` under test, so the install moves a few bytes rather than a
        // debug binary, and the device runs this build's node.
        let packages = base.join("p");
        fs::create_dir(&packages).unwrap();
        executable(
            &packages.join("hided"),
            "#!/bin/sh\nexec \"$HIDE_TEST_HIDED\" \"$@\"\n",
        );
        let events = Arc::new(Events::default());
        let slot: hide_node::ssh::PaneEventsSlot = Arc::default();
        let _ = slot.set(Arc::clone(&events) as Arc<dyn hide_node::ssh::PaneEvents>);
        let transport = hide_node::ssh::Connector::new(Some(packages))
            .with_pane_events(slot)
            .transport(&local, "contract-node", ALIAS, None)
            .unwrap();
        let consent = HostConsent {
            contract: HOST_CONSENT_CONTRACT,
            helper_root: "~/helper".to_owned(),
            cli_dir: None,
            granted_at_unix_ms: 1,
            identity: None,
        };
        let link = match transport.establish(&consent, &[], Box::new(|_| {})) {
            Ok(established) => established.host,
            Err(error) => panic!("the device's node did not start: {error:?}"),
        };
        Self {
            _root: root,
            home,
            project,
            ssh,
            transport,
            link,
            state,
            events,
        }
    }
}

fn executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// The answers a node gives for one checkout: its files, a file's bytes, its
/// Git changes and worktrees, and the account's hook state.
fn contract(link: &dyn NodeLink, project: &Path) -> Vec<Value> {
    let path = project.to_str().unwrap().to_owned();
    let opened: RootOpened = call_as(link, Call::RootOpen { root: path.clone() }, TIMEOUT).unwrap();
    let root = RootRef {
        path: path.clone(),
        identity: opened.identity,
    };
    let calls = [
        Call::List {
            root: root.clone(),
            path: String::new(),
        },
        Call::Bytes {
            root: root.clone(),
            path: "a.txt".to_owned(),
            offset: 0,
            length: 64,
        },
        Call::Changes {
            root,
            scope: String::new(),
            selected: Some("a.txt".to_owned()),
            committed: false,
            base: None,
            diffs: Vec::new(),
        },
        Call::Worktrees {
            path,
            bases: BTreeMap::new(),
            base_override: None,
        },
        Call::HookDiagnosis,
    ];
    calls
        .into_iter()
        .map(|call| {
            let name = format!("{call:?}");
            let mut answer = call_as::<Value>(link, call, TIMEOUT)
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            unbound(&mut answer);
            answer
        })
        .collect()
}

/// Drops what an answer reads from outside the node: when it was measured,
/// and which agent CLIs the answering process's own `PATH` finds.
fn unbound(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("measured_at_unix_ms");
            object.remove("memory_compatibility");
            object.values_mut().for_each(unbound);
        }
        Value::Array(items) => items.iter_mut().for_each(unbound),
        _ => {}
    }
}

#[test]
fn the_core_s_own_node_and_a_device_link_answer_alike() {
    let device = Device::start();
    let local = hide_node::Local::new(Some(device.home.clone()));
    let here = contract(&local, &device.project);
    let there = contract(&*device.link, &device.project);
    assert_eq!(here, there);
    // What the checkout holds, as written above, not as either node said.
    let listed = here[0].to_string();
    assert!(
        listed.contains("\"a.txt\"") && listed.contains("\"b.txt\""),
        "{listed}"
    );
    let bytes = here[1].to_string();
    assert!(
        bytes.contains("dHdvCg==") || bytes.contains("two\\n"),
        "{bytes}"
    );
    let changes = here[2].to_string();
    assert!(
        changes.contains("a.txt") && changes.contains("-one") && changes.contains("+two"),
        "{changes}"
    );
}

#[test]
fn a_device_s_channels_share_its_one_connection() {
    let device = Device::start();
    // The link, the install's reads and its SFTP made one connection.
    assert_eq!(device.ssh.accepted(), 1);
    // A Herdr channel, a status read and an attachment's SFTP open on it.
    device.transport.herdr_api_connector().connect().unwrap();
    let request = "8a6e0e4c-3f0b-4d2c-9a51-0c6f2b9d7e10";
    let files = [hide_node_link::attachments::AttachmentFile {
        path: "attached.txt".to_owned(),
        name: "attached.txt".to_owned(),
        bytes: b"attached\n".to_vec(),
    }];
    let staged = device
        .transport
        .stage_attachments(request, &files, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(staged.len(), 1);
    assert_eq!(fs::read(&staged[0]).unwrap(), b"attached\n");
    device.transport.remove_attachments(request, &files);
    assert!(!Path::new(&staged[0]).exists());
    assert_eq!(device.ssh.accepted(), 1);
    // Closing the link ends its node and leaves the connection to the rest.
    device.link.close("contract");
    device.transport.herdr_api_connector().connect().unwrap();
    assert_eq!(device.ssh.accepted(), 1);
}

/// A reporting call crosses the link both ways: the device's Git watch
/// reports up as it runs, a report answered with false stops it on the
/// device, and the call ends with the watch's own answer.
#[test]
fn a_device_s_reporting_call_reports_and_stops_when_told() {
    use hide_node_link::worktrees::GitWatchReport;
    let device = Device::start();
    let common = device.project.join("watched");
    fs::create_dir_all(common.join("refs/heads")).unwrap();
    let mut reports = Vec::new();
    let mut wrote = false;
    let answer = device.link.call_with_progress(
        Call::GitWatch {
            common_dirs: vec![common.to_string_lossy().into_owned()],
        },
        Duration::from_secs(30),
        &mut |report| {
            let report: GitWatchReport = serde_json::from_value(report).unwrap();
            if !wrote {
                fs::write(common.join("refs/heads/main"), "0000\n").unwrap();
                wrote = true;
            }
            let changed = matches!(report, GitWatchReport::Changed { .. });
            reports.push(report);
            !changed
        },
    );
    assert!(answer.is_ok(), "{answer:?}");
    assert!(
        matches!(reports.first(), Some(GitWatchReport::Watching { .. })),
        "{reports:?}"
    );
    assert!(
        matches!(reports.last(), Some(GitWatchReport::Changed { .. })),
        "{reports:?}"
    );
    // The link still answers after the stopped call.
    assert!(
        device
            .link
            .call(Call::Hello, Duration::from_secs(10))
            .is_ok()
    );
}

/// Waits until the node removed `path`, which it does once its input ends.
#[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
fn gone_within(path: &Path, bound: Duration) {
    let deadline = std::time::Instant::now() + bound;
    while path.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The device's pane sockets: one `bridge-*` folder per daemon's link, each
/// holding the socket a device pane's `hide` asks.
fn pane_sockets(state: &Path) -> Vec<PathBuf> {
    match fs::read_dir(state.join("workspace-bridges")) {
        Ok(entries) => entries
            .map(|entry| entry.unwrap().path().join("bootstrap.sock"))
            .filter(|socket| socket.exists())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// B18 and letter-720: the node's pane socket is the account's alone, a
/// caller the node cannot prove is refused with today's reason, and the
/// socket lives exactly as long as the link it answers for.
#[test]
fn a_device_s_pane_socket_lives_with_its_link() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    let device = Device::start();
    let sockets = pane_sockets(&device.state);
    assert_eq!(sockets.len(), 1, "{sockets:?}");
    let socket = &sockets[0];
    let mode = |path: &Path| fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(socket.parent().unwrap()), 0o700);
    assert_eq!(mode(&device.state.join("workspace-bridges")), 0o700);
    assert_eq!(mode(socket) & 0o077, 0, "{:o}", mode(socket));
    // This test process descends from no pane of the device's Herdr.
    let mut stream = UnixStream::connect(socket).unwrap();
    writeln!(
        stream,
        "{}",
        json!({"pane_id": "w1:p1", "nonce": "0".repeat(32)})
    )
    .unwrap();
    let mut answer = String::new();
    BufReader::new(&stream).read_line(&mut answer).unwrap();
    let answer: Value = serde_json::from_str(&answer).unwrap();
    assert_eq!(answer, json!({"ok": false, "reason": "pane_unavailable"}));
    device.link.close("contract");
    gone_within(socket, TIMEOUT);
    assert!(
        !socket.exists(),
        "the node kept its socket after its link closed"
    );
    assert_eq!(*device.events.closed.lock().unwrap(), ["contract-node"]);
}
