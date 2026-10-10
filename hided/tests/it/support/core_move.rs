//! Two machines on one host for a core move (PRD core-host-node-move): the
//! source, whose core moves, and the target, a device the source's core
//! dials over the fixture's own SSH server. Each is a private account under
//! /tmp with its own Herdr, state folder and fixture machine id, and its
//! HOME is declared a fixture HOME, so the target's steps start its core as
//! a detached process rather than through launchd.
//!
//! Every process here is owned by the fixture and ended by it, the target
//! core the move started included; nothing reaches the operator's Herdr,
//! state, SSH or login items.
#![allow(dead_code)]

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail, ensure};
use hide_platform::process::OwnedChild;
use serde_json::{Value, json};

use super::remote_core::Herdr;
use super::remote_delivery::renderer::Renderer;
use super::remote_delivery::{Environment, Ssh, read, successful, wait_for};

pub const SOURCE_NODE: &str = "fixture-move-source";
pub const TARGET_NODE: &str = "fixture-move-target";
/// The SSH alias the source reaches the target by, and the target's
/// registration id in the source's core.
pub const ALIAS: &str = "mini";

pub struct Machine {
    pub herdr: Herdr,
    pub state: PathBuf,
}

impl Machine {
    pub fn home(&self) -> &Path {
        &self.herdr.environment.home
    }

    pub fn project(&self) -> PathBuf {
        self.home().join("project")
    }

    /// The daemon of this machine's state folder, as it recorded itself.
    pub fn daemon(&self) -> Result<Option<Value>> {
        let path = self.state.join("hided.json");
        if !path.exists() {
            return Ok(None);
        }
        Ok(serde_json::from_slice(&read(&path)?).ok())
    }

    pub fn record(&self, name: &str) -> Result<Option<Value>> {
        let path = self.state.join(name);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_slice(&read(&path)?)?))
    }
}

pub struct Fixture {
    pub source: Machine,
    pub target: Machine,
    pub ssh: Ssh,
    pub cli: PathBuf,
    pub root: PathBuf,
    removed: bool,
    daemon: Option<OwnedChild>,
    port: u16,
    token: String,
}

fn machine_env(root: &Path, bin: &Path, node: &str) -> Result<(Environment, PathBuf, PathBuf)> {
    let socket = root.join("h.sock");
    let state = root.join("s");
    hide_platform::fs::private::create_dir_all(&state)?;
    let mut environment = Environment::new(root, &socket, bin, Some(&state))?;
    fs::write(environment.home.join(hided::env::FIXTURE_HOME_MARKER), "")?;
    environment.set("HIDE_MACHINE_ID", node);
    environment.set("HIDE_KEEP_ALIVE", "1");
    environment.set("HIDE_OPEN_COMMAND", "/usr/bin/true");
    environment.set(
        "HIDE_TAILSCALE_BIN",
        environment.home.join("absent-tailscale").into_os_string(),
    );
    let mut init = environment.command("/usr/bin/git");
    init.args(["-c", "init.defaultBranch=main", "-C"])
        .arg(environment.home.join("project"))
        .args(["init", "-q"]);
    successful(init)?;
    Ok((environment, socket, state))
}

impl Fixture {
    pub fn start() -> Result<Self> {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let bin = PathBuf::from(
            std::env::var_os("HIDE_E2E_HERDR_BIN")
                .context("set HIDE_E2E_HERDR_BIN to the pinned binary")?,
        );
        let source_cli = std::env::var_os("HIDE_E2E_CLI_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| repository.join("target/debug"));
        for name in ["hide", "hided", "hide-agent-hooks"] {
            ensure!(
                source_cli.join(name).is_file(),
                "build this worktree's CLI binaries before core_move"
            );
        }
        // Under the system's temporary folder, which the fixture starter
        // requires of a HOME, and short enough for a socket path.
        let root = tempfile::Builder::new()
            .prefix("hcm-")
            .tempdir_in("/tmp")?
            .keep();
        let root = hide_platform::fs::identity::canonical(&root)?;
        let (mut source_env, source_socket, source_state) =
            machine_env(&root.join("m"), &bin, SOURCE_NODE)?;
        let (mut target_env, target_socket, target_state) =
            machine_env(&root.join("c"), &bin, TARGET_NODE)?;
        target_env.set("HIDE_CORE_STARTER", "fixture");
        // The target's reads find `herdr` where a real install puts it.
        fs::create_dir_all(target_env.home.join(".local/bin"))?;
        std::os::unix::fs::symlink(&bin, target_env.home.join(".local/bin/herdr"))?;
        // A shipped build carries no debug data; the helper upload stays
        // inside its bound with a copy of the same kind.
        let cli = root.join("cli");
        fs::create_dir(&cli)?;
        for name in ["hide", "hided", "hide-agent-hooks"] {
            let staged = cli.join(name);
            fs::copy(source_cli.join(name), &staged)?;
            hide_platform::fs::private::restrict_to_owner(&staged)?;
            let mut strip = source_env.command("/usr/bin/strip");
            strip.arg(&staged);
            successful(strip)?;
            #[cfg(target_os = "macos")]
            {
                let mut sign = source_env.command("/usr/bin/codesign");
                sign.args(["--force", "--sign", "-"]).arg(&staged);
                successful(sign)?;
            }
        }
        let ssh_dir = source_env.home.join(".ssh");
        fs::create_dir_all(&ssh_dir)?;
        for name in ["host", "client"] {
            let mut command = source_env.command("/usr/bin/ssh-keygen");
            command
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(ssh_dir.join(name));
            successful(command)?;
        }
        let source_herdr = Herdr::start(
            &root.join("m"),
            source_env.clone(),
            source_socket.clone(),
            &bin,
        )?;
        let target_herdr = Herdr::start(
            &root.join("c"),
            target_env.clone(),
            target_socket.clone(),
            &bin,
        )?;
        let ssh = Ssh::start(
            target_env.clone(),
            target_socket.clone(),
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
        source_env.set(
            "HIDE_HOST_HELPER_ROOT",
            target_env.home.join("helper").into_os_string(),
        );
        source_env.set(
            "HIDE_HOST_CLI_DIR",
            target_env.home.join("bin").into_os_string(),
        );
        let mut fixture = Self {
            source: Machine {
                herdr: source_herdr,
                state: source_state,
            },
            target: Machine {
                herdr: target_herdr,
                state: target_state,
            },
            ssh,
            cli,
            root,
            removed: false,
            daemon: None,
            port: 0,
            token: String::new(),
        };
        fixture.source.herdr.environment = source_env;
        fixture.start_source()?;
        fixture.event(
            "create_workspace",
            json!({"path": fixture.source.project(), "label": "Source fixture", "initialize_git": false}),
        )?;
        fixture.event(
            "register_device",
            json!({"id": ALIAS, "label": "Target fixture", "ssh_alias": ALIAS, "herdr_socket_path": target_socket, "host_consent": true}),
        )?;
        fixture.device_ready()?;
        fixture.event(
            "create_workspace",
            json!({"device_id": ALIAS, "path": fixture.target.project(), "label": "Target fixture", "initialize_git": false}),
        )?;
        wait_for("both machines' projects registered", || {
            let projects = fixture.projects()?;
            Ok((projects.len() == 2).then_some(()))
        })?;
        Ok(fixture)
    }

    /// Waits until the source's core holds a ready link to the target.
    pub fn device_ready(&self) -> Result<()> {
        wait_for("the target's helper ready", || {
            let snapshot = self.snapshot()?;
            let host = snapshot
                .pointer("/navigator/devices")
                .and_then(Value::as_array)
                .and_then(|rows| rows.iter().find(|row| row["id"] == ALIAS))
                .and_then(|row| row.get("host"))
                .cloned();
            if let Some(host) = &host
                && matches!(
                    host["state"].as_str(),
                    Some("unavailable" | "unsupported" | "identity_changed")
                )
            {
                bail!("the target's helper failed: {}", host["message"]);
            }
            Ok(host
                .is_some_and(|host| host["state"] == "ready")
                .then_some(()))
        })
    }

    /// Starts the source machine's hided, which runs its core.
    pub fn start_source(&mut self) -> Result<()> {
        ensure!(self.daemon.is_none(), "the source's hided already runs");
        match fs::remove_file(self.source.state.join("hided.json")) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        let log = File::options()
            .create(true)
            .append(true)
            .open(self.root.join("source-hided.log"))?;
        let mut command = self
            .source
            .herdr
            .environment
            .command(self.cli.join("hided"));
        command
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        self.daemon = Some(OwnedChild::spawn(&mut command)?);
        let state: Value = wait_for("the source's hided state", || self.source.daemon())?;
        self.port = state["port"].as_u64().context("source port")? as u16;
        self.token = state["token"].as_str().context("source token")?.to_owned();
        Ok(())
    }

    /// The source's window address: it stays the same through a move.
    pub fn window(&self) -> (u16, String) {
        (self.port, self.token.clone())
    }

    pub fn snapshot(&self) -> Result<Value> {
        Ok(Renderer::connect(self.port, &self.token)?
            .snapshot()
            .clone())
    }

    pub fn event(&self, kind: &str, payload: Value) -> Result<()> {
        Renderer::connect(self.port, &self.token)?.event(kind, payload)
    }

    /// The registered projects the source's window shows, as (machine,
    /// path), in order.
    pub fn projects(&self) -> Result<Vec<(String, String)>> {
        let snapshot = self.snapshot()?;
        let mut projects: Vec<(String, String)> = snapshot
            .pointer("/ui_state/workspace_registrations")
            .and_then(Value::as_array)
            .context("registrations")?
            .iter()
            .map(|row| {
                (
                    row["device_id"].as_str().unwrap_or_default().to_owned(),
                    row["path"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        projects.sort();
        Ok(projects)
    }

    /// The source's health, as a window's host reads it.
    pub fn health(&self) -> Result<Value> {
        let answer = ureq::get(&format!("http://127.0.0.1:{}/health", self.port))
            .call()
            .context("source health")?
            .into_body()
            .read_to_string()?;
        Ok(serde_json::from_str(&answer)?)
    }

    /// Waits for the source's move journal to reach `phase`.
    pub fn journal_until(&self, phase: &str) -> Result<Value> {
        wait_for(&format!("the move journal at {phase}"), || {
            let journal = self.source.record("core-move.json")?;
            if let Some(journal) = &journal
                && journal["phase"]["phase"] == "rolled_back"
                && phase != "rolled_back"
            {
                bail!("the move rolled back: {journal}");
            }
            Ok(journal.filter(|journal| journal["phase"]["phase"] == phase))
        })
    }

    /// Ends the source's hided at once, as a crash or a power cut would.
    pub fn kill_source(&mut self) -> Result<()> {
        let mut daemon = self
            .daemon
            .take()
            .context("the source's hided is not running")?;
        daemon.kill_tree()?;
        wait_for("the source's hided confirmed exit", || {
            Ok(daemon.try_wait()?.map(|_| ()))
        })
    }

    /// Waits until the source's hided ends on its own, within `bound`, and
    /// answers how it ended.
    pub fn source_ended_within(
        &mut self,
        bound: std::time::Duration,
    ) -> Result<std::process::ExitStatus> {
        let daemon = self
            .daemon
            .as_mut()
            .context("the source's hided is not running")?;
        let status =
            super::remote_delivery::wait_within("the source's hided ended", bound, || {
                Ok(daemon.try_wait()?)
            })?;
        self.daemon = None;
        Ok(status)
    }

    /// The role the source's window reaches: `core`, `node` or `moving`.
    pub fn role(&self) -> Result<String> {
        Ok(self.health()?["role"]
            .as_str()
            .context("health role")?
            .to_owned())
    }

    /// Waits until `machine`'s core logs a record of `kind`, and answers it.
    pub fn logged(&self, machine: &Machine, kind: &str) -> Result<Value> {
        let path = machine.state.join("Logs/core.jsonl");
        wait_for(&format!("a {kind} record"), || {
            let Ok(bytes) = read(&path) else {
                return Ok(None);
            };
            Ok(String::from_utf8_lossy(&bytes)
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .find(|record| record["kind"] == kind))
        })
    }

    /// The pid of the core the move started on the target, if one runs.
    pub fn target_core(&self) -> Result<Option<u32>> {
        Ok(self.target.daemon()?.and_then(|state| {
            let pid = state["pid"].as_u64()? as u32;
            hide_platform::process::is_alive(pid).then_some(pid)
        }))
    }

    /// Holds the target's handover record lock, as another change of it
    /// would: every step and core that changes the record waits for it.
    pub fn hold_target_handover(&self) -> Result<hide_platform::fs::lock::Lock> {
        let file = hide_platform::fs::private::open_or_create_file(
            &self.target.state.join("core-handover.lock"),
        )?;
        match hide_platform::fs::lock::lock_file(
            file,
            hide_platform::fs::lock::Mode::Exclusive,
            std::time::Duration::from_secs(5),
            &|| false,
        )? {
            hide_platform::fs::lock::Waited::Locked(lock) => Ok(lock),
            _ => bail!("the target's handover lock stayed held"),
        }
    }

    /// Runs the target's hided as its login item would after a login or a
    /// keep-alive restart, and answers how it ended.
    pub fn start_target_hided(&self) -> Result<std::process::ExitStatus> {
        let log = File::options()
            .create(true)
            .append(true)
            .open(self.root.join("target-hided.log"))?;
        let mut command = self
            .target
            .herdr
            .environment
            .command(self.cli.join("hided"));
        command
            .arg("core-login")
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        let mut child = OwnedChild::spawn(&mut command)?;
        wait_for("the target's hided ended", || Ok(child.try_wait()?))
    }

    /// The core logged on `machine`, for a failure's message.
    pub fn log_tail(&self, machine: &Machine) -> String {
        let path = machine.state.join("Logs/core.jsonl");
        let text = read(&path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        let text = text.unwrap_or_else(|error| format!("no log at {}: {error}", path.display()));
        text.lines().rev().take(40).collect::<Vec<_>>().join("\n")
    }

    pub fn stop(&mut self) -> Result<()> {
        if let Some(mut daemon) = self.daemon.take() {
            daemon.kill_tree()?;
            wait_for("the source's hided confirmed exit", || {
                Ok(daemon.try_wait()?.map(|_| ()))
            })?;
        }
        // The target's core is the move's, started detached by its step.
        if let Some(pid) = self.target_core()? {
            hide_platform::process::kill_tree(pid)?;
            wait_for("the target's core confirmed exit", || {
                Ok((!hide_platform::process::is_alive(pid)).then_some(()))
            })?;
        }
        self.ssh.stop()?;
        self.target.herdr.stop()?;
        self.source.herdr.stop()?;
        Ok(())
    }

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
            eprintln!(
                "core move fixture cleanup failed, evidence kept at {}: {error}",
                self.root.display()
            );
        }
    }
}
