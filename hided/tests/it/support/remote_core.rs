//! Two machines on one host, for the node that dials its core (PRD
//! core-host-node-remote-core): the core machine is a private account behind
//! the fixture's own SSH server, with its own Herdr, state folder and core;
//! the screen machine is a second private account with its own Herdr. The
//! core runs under a fixture machine id, so the two are two nodes.
//!
//! Every process here is owned by the fixture and ended by it; nothing
//! reaches the operator's Herdr, state or SSH.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, ensure};
use hide_platform::process::OwnedChild;
use serde_json::{Value, json};

use super::remote_delivery::renderer::Renderer;
use super::remote_delivery::{Environment, Ssh, capture, read, successful, wait_for};

/// The core machine's node id.
pub const CORE_NODE: &str = "fixture-core-machine";
pub const ALIAS: &str = "core-fixture";

/// A private Herdr server of one account.
pub struct Herdr {
    pub environment: Environment,
    pub socket: PathBuf,
    binary: PathBuf,
    server: Option<OwnedChild>,
}

impl Herdr {
    fn start(
        root: &Path,
        environment: Environment,
        socket: PathBuf,
        binary: &Path,
    ) -> Result<Self> {
        let log = File::create(root.join("herdr.log"))?;
        let mut command = environment.command(binary);
        command
            .arg("server")
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        let server = OwnedChild::spawn(&mut command)?;
        let herdr = Self {
            environment,
            socket,
            binary: binary.to_owned(),
            server: Some(server),
        };
        wait_for("private Herdr socket", || {
            let mut command = herdr.environment.command(&herdr.binary);
            command.args(["api", "snapshot"]);
            let output = capture(command)?;
            Ok(output.status.success().then_some(()))
        })?;
        Ok(herdr)
    }

    /// Runs `herdr <args>` against this server and answers its JSON.
    pub fn run(&self, args: &[&str]) -> Result<Value> {
        let mut command = self.environment.command(&self.binary);
        command.args(args);
        serde_json::from_slice(&successful(command)?).context("private Herdr answer JSON")
    }

    /// A Herdr workspace at `folder`, and its root pane.
    pub fn workspace_at(&self, folder: &Path) -> Result<String> {
        let created = self.run(&[
            "workspace",
            "create",
            "--label",
            "fixture",
            "--cwd",
            folder.to_str().context("fixture path UTF-8")?,
            "--focus",
        ])?;
        Ok(created
            .pointer("/result/root_pane/pane_id")
            .and_then(Value::as_str)
            .context("private Herdr root pane")?
            .to_owned())
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(mut server) = self.server.take() {
            let mut command = self.environment.command(&self.binary);
            command.args(["server", "stop"]);
            let stopped = capture(command)?;
            if !stopped.status.success() {
                server.kill_tree()?;
            }
            wait_for("private Herdr confirmed exit", || {
                Ok(server.try_wait()?.map(|_| ()))
            })?;
        }
        Ok(())
    }
}

pub struct Fixture {
    /// The core machine: its account, Herdr and state folder.
    pub core: Herdr,
    pub core_state: PathBuf,
    /// The screen machine: its account and Herdr.
    pub screen: Herdr,
    pub ssh: Ssh,
    pub hided: PathBuf,
    pub root: PathBuf,
    removed: bool,
    _ipc: tempfile::TempDir,
    daemon: Option<OwnedChild>,
    /// The screen machine's hided in the node role, once started.
    node: Option<OwnedChild>,
    port: u16,
    token: String,
}

impl Fixture {
    pub fn start() -> Result<Self> {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let bin = PathBuf::from(
            std::env::var_os("HIDE_E2E_HERDR_BIN")
                .context("set HIDE_E2E_HERDR_BIN to the pinned binary")?,
        );
        let cli = std::env::var_os("HIDE_E2E_CLI_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| repository.join("target/debug"));
        let hided = cli.join("hided");
        ensure!(
            hided.is_file(),
            "build this worktree's hided before remote_core"
        );
        let artifacts = repository.join("agents/runs");
        fs::create_dir_all(&artifacts)?;
        let root = tempfile::Builder::new()
            .prefix("rc-")
            .tempdir_in(artifacts)?
            .keep();
        // Sockets and the core's state folder stay short enough for a Unix
        // socket path.
        let ipc = tempfile::Builder::new().prefix("rc-").tempdir_in("/tmp")?;
        let core_state = ipc.path().join("c");
        hide_platform::fs::private::create_dir_all(&core_state)?;
        let core_socket = ipc.path().join("c.sock");
        let screen_socket = ipc.path().join("s.sock");
        let mut core_env = Environment::new(&root.join("c"), &core_socket, &bin, None)?;
        let screen_env = Environment::new(&root.join("s"), &screen_socket, &bin, None)?;
        core_env.set("HIDE_MACHINE_ID", CORE_NODE);
        core_env.set("HIDE_STATE_DIR", core_state.as_os_str());
        core_env.set("HIDE_KEEP_ALIVE", "1");
        core_env.set("HIDE_OPEN_COMMAND", "/usr/bin/true");
        core_env.set(
            "HIDE_TAILSCALE_BIN",
            core_env.home.join("absent-tailscale").into_os_string(),
        );
        for environment in [&core_env, &screen_env] {
            let mut command = environment.command("/usr/bin/git");
            command
                .args(["-c", "init.defaultBranch=main", "-C"])
                .arg(environment.home.join("project"))
                .args(["init", "-q"]);
            successful(command)?;
        }
        let ssh_dir = screen_env.home.join(".ssh");
        fs::create_dir_all(&ssh_dir)?;
        for name in ["host", "client"] {
            let mut command = screen_env.command("/usr/bin/ssh-keygen");
            command
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(ssh_dir.join(name));
            successful(command)?;
        }
        let core = Herdr::start(&root.join("c"), core_env.clone(), core_socket.clone(), &bin)?;
        let screen = Herdr::start(&root.join("s"), screen_env, screen_socket, &bin)?;
        let ssh = Ssh::start(
            core_env.clone(),
            core_socket,
            &ssh_dir.join("host"),
            &ssh_dir.join("client"),
        )?;
        let public = fs::read_to_string(ssh_dir.join("host.pub"))?;
        fs::write(
            ssh_dir.join("known_hosts"),
            format!("[127.0.0.1]:{} {}\n", ssh.port, public.trim()),
        )?;
        fs::write(
            ssh_dir.join("config"),
            format!(
                "Host {ALIAS}\n  HostName 127.0.0.1\n  Port {}\n  User fixture\n  IdentityFile {}\n  IdentityAgent none\n",
                ssh.port,
                ssh_dir.join("client").display(),
            ),
        )?;
        let log = File::create(root.join("core-hided.log"))?;
        let mut command = core_env.command(&hided);
        command
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        let daemon = OwnedChild::spawn(&mut command)?;
        let mut fixture = Self {
            core,
            core_state: core_state.clone(),
            screen,
            ssh,
            hided,
            root,
            removed: false,
            _ipc: ipc,
            daemon: Some(daemon),
            node: None,
            port: 0,
            token: String::new(),
        };
        let daemon_state: Value = wait_for("private core hided state", || {
            let path = core_state.join("hided.json");
            if !path.exists() {
                return Ok(None);
            }
            Ok(serde_json::from_slice(&read(&path)?).ok())
        })?;
        fixture.port = daemon_state["port"].as_u64().context("core port")? as u16;
        fixture.token = daemon_state["token"]
            .as_str()
            .context("core token")?
            .to_owned();
        wait_for("core attach socket", || {
            Ok(core_state.join("node-attach-socket").exists().then_some(()))
        })?;
        Ok(fixture)
    }

    /// Records the core machine as this screen machine's core, as the move
    /// will (layer 5).
    pub fn placement(&self) -> hided::placement::Placement {
        hided::placement::Placement {
            alias: ALIAS.to_owned(),
            node: CORE_NODE.to_owned(),
            program: self.hided.display().to_string(),
            state_dir: Some(self.core_state.display().to_string()),
        }
    }

    pub fn screen_home(&self) -> &Path {
        &self.screen.environment.home
    }

    /// Starts the screen machine's hided with the core machine recorded as
    /// its core: it runs in the node role. Answers its port and token.
    pub fn start_node(&mut self) -> Result<(u16, String)> {
        self.start_node_with(&self.hided.clone())
    }

    /// The screen machine's state folder, which records its core.
    pub fn node_state(&self) -> PathBuf {
        self._ipc.path().join("s")
    }

    /// `program` run as the screen machine's account, on its state folder.
    pub fn screen_command(&self, program: &Path) -> std::process::Command {
        let mut environment = self.screen.environment.clone();
        environment.set("HIDE_STATE_DIR", self.node_state().as_os_str());
        environment.set("HIDE_OPEN_COMMAND", "/usr/bin/true");
        environment.command(program)
    }

    /// `program` run as the core machine's account, on the core's state
    /// folder.
    pub fn core_command(&self, program: &Path) -> std::process::Command {
        let mut environment = self.core.environment.clone();
        environment.set("HIDE_STATE_DIR", self.core_state.as_os_str());
        environment.set("HIDE_OPEN_COMMAND", "/usr/bin/true");
        environment.command(program)
    }

    /// Sends the screen machine's hided `signal` and waits for it to end.
    pub fn signal_node(&mut self, signal: i32) -> Result<()> {
        let mut node = self.node.take().context("no node hided runs")?;
        let pid = i32::try_from(node.id())?;
        // SAFETY: the pid is the fixture's own unreaped child.
        ensure!(
            unsafe { libc::kill(pid, signal) } == 0,
            "the node was not signalled"
        );
        wait_for("private node hided confirmed exit", || {
            Ok(node.try_wait()?.map(|_| ()))
        })?;
        Ok(())
    }

    /// Sends the screen machine's hided `signal`, leaving it the fixture's.
    pub fn signal_running_node(&self, signal: i32) -> Result<()> {
        let node = self.node.as_ref().context("no node hided runs")?;
        let pid = i32::try_from(node.id())?;
        // SAFETY: the pid is the fixture's own unreaped child.
        ensure!(
            unsafe { libc::kill(pid, signal) } == 0,
            "the node was not signalled"
        );
        Ok(())
    }

    /// The attach role processes running for the core's state folder, as
    /// the process table lists them.
    pub fn attach_processes(&self) -> Result<Vec<String>> {
        let mut command = std::process::Command::new("/bin/ps");
        command.args(["-axo", "pid=,command="]);
        let listed = String::from_utf8(successful(command)?)?;
        let wanted = format!("attach --state-dir {}", self.core_state.display());
        Ok(listed
            .lines()
            .filter(|line| line.contains(&wanted) && !line.contains("/bin/ps"))
            .map(str::to_owned)
            .collect())
    }

    /// Ends the core machine's hided: its machine still answers SSH, and
    /// its attach socket's record stays behind.
    pub fn stop_core(&mut self) -> Result<()> {
        if let Some(mut daemon) = self.daemon.take() {
            daemon.kill_tree()?;
            wait_for("private core hided confirmed exit", || {
                Ok(daemon.try_wait()?.map(|_| ()))
            })?;
        }
        Ok(())
    }

    /// [`Fixture::start_node`] with the `hided` at `hided`.
    pub fn start_node_with(&mut self, hided: &Path) -> Result<(u16, String)> {
        let state = self.node_state();
        hide_platform::fs::private::create_dir_all(&state)?;
        let placement = self.placement();
        let record = hided::placement::record_path(&state);
        fs::write(
            &record,
            serde_json::to_vec(&json!({
                "alias": placement.alias,
                "node": placement.node,
                "program": placement.program,
                "state_dir": placement.state_dir,
            }))?,
        )?;
        hide_platform::fs::private::restrict_to_owner(&record)?;
        // A node killed before it could clean up leaves its state behind;
        // this start's state is the one waited for.
        match fs::remove_file(state.join("hided.json")) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        let mut environment = self.screen.environment.clone();
        environment.set("HIDE_STATE_DIR", state.as_os_str());
        environment.set("HIDE_KEEP_ALIVE", "1");
        environment.set("HIDE_OPEN_COMMAND", "/usr/bin/true");
        let log = File::create(self.root.join("node-hided.log"))?;
        let mut command = environment.command(hided);
        command
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        self.node = Some(OwnedChild::spawn(&mut command)?);
        let daemon_state: Value = wait_for("private node hided state", || {
            let path = state.join("hided.json");
            if !path.exists() {
                return Ok(None);
            }
            Ok(serde_json::from_slice(&read(&path)?).ok())
        })?;
        let port = daemon_state["port"].as_u64().context("node port")? as u16;
        let token = daemon_state["token"]
            .as_str()
            .context("node token")?
            .to_owned();
        Ok((port, token))
    }

    /// The node role's diagnostic rows of `component` and `kind`.
    pub fn node_log(&self, component: &str, kind: &str) -> Result<Vec<Value>> {
        log_rows(&self._ipc.path().join("s/Logs/core.jsonl"), component, kind)
    }

    /// The core's loopback port and screen token.
    pub fn core_screen(&self) -> (u16, String) {
        (self.port, self.token.clone())
    }

    pub fn core_home(&self) -> &Path {
        &self.core.environment.home
    }

    pub fn snapshot(&self) -> Result<Value> {
        Ok(Renderer::connect(self.port, &self.token)?
            .snapshot()
            .clone())
    }

    pub fn event(&self, kind: &str, payload: Value) -> Result<()> {
        Renderer::connect(self.port, &self.token)?.event(kind, payload)
    }

    /// The core's device row for `node`, once it is in the snapshot.
    pub fn device(&self, node: &str) -> Result<Option<Value>> {
        Ok(self
            .snapshot()?
            .pointer("/navigator/devices")
            .and_then(Value::as_array)
            .and_then(|rows| rows.iter().find(|row| row["id"] == node))
            .cloned())
    }

    /// The core's diagnostic rows of `component` and `kind`.
    pub fn core_log(&self, component: &str, kind: &str) -> Result<Vec<Value>> {
        log_rows(&self.core_state.join("Logs/core.jsonl"), component, kind)
    }

    /// Whether `text` is in anything either daemon logged: its diagnostic
    /// log or its standard error.
    pub fn logs_mention(&self, text: &str) -> Result<bool> {
        for path in [
            self.core_state.join("Logs/core.jsonl"),
            self._ipc.path().join("s/Logs/core.jsonl"),
            self.root.join("core-hided.log"),
            self.root.join("node-hided.log"),
        ] {
            if path.exists() && String::from_utf8_lossy(&read(&path)?).contains(text) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn create_workspace_on(&self, node: &str, path: &Path) -> Result<()> {
        self.event(
            "create_workspace",
            json!({"device_id": node, "path": path, "label": "Screen fixture", "initialize_git": false}),
        )
    }

    pub fn stop(&mut self) -> Result<()> {
        if let Some(mut node) = self.node.take() {
            node.kill_tree()?;
            wait_for("private node hided confirmed exit", || {
                Ok(node.try_wait()?.map(|_| ()))
            })?;
        }
        self.stop_core()?;
        self.ssh.stop()?;
        self.core.stop()?;
        self.screen.stop()?;
        Ok(())
    }

    /// The journey passed and every owned process confirmed its exit: the
    /// run directory holds nothing anyone needs.
    pub fn remove_run_dir(&mut self) -> Result<()> {
        self.stop()?;
        fs::remove_dir_all(&self.root)
            .with_context(|| format!("remove {}", self.root.display()))?;
        self.removed = true;
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.removed {
            return;
        }
        if let Err(error) = self.stop() {
            eprintln!("private remote core fixture cleanup failed, evidence retained: {error}");
        }
    }
}

fn log_rows(path: &Path, component: &str, kind: &str) -> Result<Vec<Value>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(String::from_utf8(read(path)?)?
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|row| row["component"] == component && row["kind"] == kind)
        .collect())
}
