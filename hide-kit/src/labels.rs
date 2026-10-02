//! Retires the agent labels Herdr plugin, `hide.agent-context-labels` (PRD
//! labels-in-hided D-12).
//!
//! Labels are made by hided's core now, so every kit pass takes the plugin
//! off the machine: its Herdr link however it was installed, the watcher it
//! left running, the kit's copy, and the plugin's state folder. A watcher
//! left behind would analyze the same turns again. Herdr's server, the agent
//! sessions, Herdr's per-plugin config folder and the operator's
//! `config.toml` are never touched.
//!
//! A part that could not be taken out is reported and tried again on the next
//! pass; the copy and the state folder stay until the link is gone, because
//! they are how a later pass knows the plugin was ever here.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hide_herdr_client::wire::success_response::{InstalledPluginInfo, PluginSourceKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{KitTarget, process};

pub const LABELS_PLUGIN_ID: &str = "hide.agent-context-labels";

/// The standalone hcoord Herdr plugin from before hcoord became part of the
/// kit (PRD hide-home-layout D-07, D-14). A startup hook it left registered
/// would converge the same daemon label on another build.
pub const HCOORD_PLUGIN_ID: &str = "hide.hcoord";

const HERDR_DEADLINE: Duration = Duration::from_secs(5);
const UNINSTALL_DEADLINE: Duration = Duration::from_secs(20);
const LSOF_DEADLINE: Duration = Duration::from_secs(5);
/// How long a watcher has to end on SIGTERM before it is killed.
const WATCHER_EXIT_DEADLINE: Duration = Duration::from_secs(5);

/// What retiring the plugin did on one machine.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Retirement {
    /// Each part found and taken out, in a few words.
    #[serde(default)]
    pub removed: Vec<String>,
    /// Why a part stayed; the next kit pass tries again.
    #[serde(default)]
    pub failures: Vec<String>,
}

impl Retirement {
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.failures.is_empty()
    }
}

/// Where the kit kept the copy Herdr linked.
pub fn labels_home(home: &Path) -> PathBuf {
    crate::record::kit_state_dir(home)
        .join("plugins")
        .join("agent-context-labels")
}

/// The plugin's own state folder, which the core imports once on this Mac
/// before the kit runs (D-11).
pub fn plugin_state_dir(home: &Path) -> PathBuf {
    home.join(".local/state/hide.agent-context-labels")
}

pub(crate) fn retire(target: &KitTarget) -> Retirement {
    let mut outcome = Retirement::default();
    let copy = labels_home(&target.home);
    let state = plugin_state_dir(&target.home);
    // Nothing of the plugin is left: no Herdr call on an ordinary launch.
    if !copy.exists() && !state.exists() {
        return outcome;
    }
    let unlinked = match unlink(target, LABELS_PLUGIN_ID) {
        Ok(Some(source)) => {
            outcome
                .removed
                .push(format!("Herdr plugin link ({source})"));
            true
        }
        Ok(None) => true,
        Err(reason) => {
            outcome.failures.push(reason);
            false
        }
    };
    // The lock is the only handle on a running watcher: the folders stay
    // until it is free, so a pass that could not stop the watcher leaves
    // the next pass a way to find it.
    let stopped = match stop_watchers(target, &state.join("watcher.lock")) {
        Ok(stopped) => {
            outcome.removed.extend(
                stopped
                    .into_iter()
                    .map(|pid| format!("watcher process {pid}")),
            );
            true
        }
        Err(reason) => {
            outcome.failures.push(reason);
            false
        }
    };
    if unlinked && stopped {
        for (folder, name) in [(&copy, "kit copy"), (&state, "state folder")] {
            match std::fs::remove_dir_all(folder) {
                Ok(()) => outcome.removed.push(name.to_owned()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => outcome.failures.push(format!(
                    "the plugin's {name} could not be removed: {}",
                    error.kind()
                )),
            }
        }
    }
    outcome
}

fn request(target: &KitTarget, method: &str, params: Value) -> Result<Value, String> {
    hide_herdr_client::request_with_timeout(&target.herdr_socket, method, params, HERDR_DEADLINE)
        .map_err(|error| {
            // A transport message can carry the socket's path (B20); the
            // remote code is the part a reader needs.
            let reason = match &error {
                hide_herdr_client::ApiError::Remote { code, .. } => code.as_str(),
                hide_herdr_client::ApiError::Transport(_) => "transport",
                hide_herdr_client::ApiError::Malformed(_) => "malformed",
            };
            format!("Herdr on this machine did not answer {method}: {reason}")
        })
}

/// Takes the standalone hcoord plugin out of Herdr on every pass: nothing on
/// disk says whether it was ever linked, and a Herdr that does not answer
/// leaves it for the next pass.
pub(crate) fn retire_hcoord_plugin(target: &KitTarget) -> Retirement {
    let mut outcome = Retirement::default();
    match unlink(target, HCOORD_PLUGIN_ID) {
        Ok(Some(source)) => outcome
            .removed
            .push(format!("hcoord Herdr plugin link ({source})")),
        Ok(None) => {}
        Err(reason) => outcome.failures.push(reason),
    }
    outcome
}

/// Takes a plugin out of Herdr however it was installed. Returns where it
/// came from, or `None` when Herdr has no such plugin.
fn unlink(target: &KitTarget, plugin_id: &str) -> Result<Option<&'static str>, String> {
    let result = request(target, "plugin.list", json!({ "plugin_id": plugin_id }))?;
    let plugins = result
        .get("plugins")
        .cloned()
        .ok_or_else(|| "Herdr's plugin list had no plugins".to_owned())?;
    let plugins: Vec<InstalledPluginInfo> = serde_json::from_value(plugins)
        .map_err(|error| format!("Herdr's plugin list could not be read: {error}"))?;
    let Some(plugin) = plugins
        .into_iter()
        .find(|plugin| plugin.plugin_id == plugin_id)
    else {
        return Ok(None);
    };
    match plugin.source.kind {
        PluginSourceKind::Github => {
            uninstall_managed(target, plugin_id)?;
            Ok(Some("GitHub"))
        }
        PluginSourceKind::Local => {
            request(target, "plugin.unlink", json!({ "plugin_id": plugin_id }))?;
            Ok(Some("linked folder"))
        }
    }
}

/// A GitHub install is Herdr's managed checkout, which only the `herdr` CLI
/// takes out; unlinking it through the socket would leave the checkout.
fn uninstall_managed(target: &KitTarget, plugin_id: &str) -> Result<(), String> {
    let herdr = target.herdr_bin.as_deref().ok_or_else(|| {
        format!("{plugin_id} is installed from GitHub and no herdr command was found to remove it")
    })?;
    let mut env = vec![(
        "HERDR_SOCKET_PATH".to_owned(),
        target.herdr_socket.display().to_string(),
    )];
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        env.push((
            "XDG_CONFIG_HOME".to_owned(),
            config.to_string_lossy().into_owned(),
        ));
    }
    let finished = process::run(
        herdr,
        &["plugin", "uninstall", plugin_id],
        &env,
        &target.home,
        UNINSTALL_DEADLINE,
        &target.stop,
    )?;
    if finished.succeeded() {
        Ok(())
    } else {
        // Its stderr can name the operator's folders (B20): the exit code
        // is the reason that reaches the log.
        Err(format!(
            "herdr could not remove the GitHub copy of {plugin_id}: exit {:?}",
            finished.code
        ))
    }
}

/// Ends every process holding this home's watcher lock. A watcher is found by
/// the lock it holds rather than by its executable, which a swap renames to
/// `.old-<pid>` and a GitHub install keeps elsewhere; the lock is under the
/// target's home, so a test home never reaches the operator's watcher.
fn stop_watchers(target: &KitTarget, lock: &Path) -> Result<Vec<i32>, String> {
    if !lock.exists() || !lock_is_held(lock)? {
        return Ok(Vec::new());
    }
    // A daemon started from the Dock may have no `/usr/sbin` on its PATH.
    let lsof = ["/usr/sbin/lsof", "/usr/bin/lsof"]
        .into_iter()
        .map(Path::new)
        .find(|path| path.is_file())
        .unwrap_or(Path::new("lsof"));
    let finished = process::run(
        lsof,
        &["-t", &lock.display().to_string()],
        &[],
        &target.home,
        LSOF_DEADLINE,
        &target.stop,
    )
    .map_err(|reason| format!("the labels watcher could not be found: {reason}"))?;
    let pids: Vec<i32> = finished
        .stdout
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .filter(|pid| *pid > 1 && *pid != std::process::id() as i32)
        .collect();
    if pids.is_empty() {
        return Err("the labels watcher holds its lock but lsof named no process".to_owned());
    }
    for pid in &pids {
        // SAFETY: kill with a pid and a signal number has no memory effects.
        unsafe { libc::kill(*pid, libc::SIGTERM) };
    }
    let started = Instant::now();
    while pids.iter().any(|pid| alive(*pid)) && started.elapsed() < WATCHER_EXIT_DEADLINE {
        std::thread::sleep(Duration::from_millis(50));
    }
    for pid in pids.iter().filter(|pid| alive(**pid)) {
        // SAFETY: as above.
        unsafe { libc::kill(*pid, libc::SIGKILL) };
    }
    // Stopped means the lock is free, whatever the signals answered: a pid
    // that survived, or a holder lsof did not name, keeps it.
    let started = Instant::now();
    // A lock file that went with its watcher is as free as an unheld one.
    while lock.exists() && lock_is_held(lock)? {
        if started.elapsed() >= WATCHER_EXIT_DEADLINE {
            return Err(format!(
                "the labels watcher still holds its lock after {} signalled process(es)",
                pids.len()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(pids)
}

/// Whether some process holds the watcher's exclusive lock: taking it for a
/// moment answers without asking lsof on the ordinary pass where none runs.
fn lock_is_held(lock: &Path) -> Result<bool, String> {
    use std::os::fd::AsRawFd;
    let file = std::fs::File::open(lock)
        .map_err(|error| format!("the watcher lock could not be opened: {}", error.kind()))?;
    // SAFETY: flock on a descriptor `file` owns; dropping `file` releases it.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(false);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(true)
    } else {
        Err(format!(
            "the watcher lock could not be checked: {}",
            error.kind()
        ))
    }
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}
