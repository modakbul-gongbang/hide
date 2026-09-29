//! The agent labels Herdr plugin, `hide.agent-context-labels`.
//!
//! The build ships the plugin prebuilt in `<kit_dir>/agent-context-labels/`
//! (a manifest with no build step, its scripts and the release binary). The
//! kit copies it to `~/.hide/kit/plugins/agent-context-labels/` and links
//! that copy in the machine's Herdr, because Herdr keeps a linked plugin by
//! its resolved path and a build folder does not outlive the next update.
//!
//! The same plugin installed another way - from GitHub, or linked from a
//! checkout - is replaced, since two copies of one id cannot both run
//! (B9). Herdr's per-plugin config folder is Herdr's and is never touched
//! (D-15).

use std::path::{Path, PathBuf};
use std::time::Duration;

use hide_herdr_client::wire::success_response::{InstalledPluginInfo, PluginSourceKind};
use serde_json::{Value, json};

use crate::{KitTarget, Observed, RemoveOutcome, payload, process};

pub const LABELS_PLUGIN_ID: &str = "hide.agent-context-labels";

/// The build's folder name for the plugin, under the kit folder.
const PACKAGED: &str = "agent-context-labels";

const HERDR_DEADLINE: Duration = Duration::from_secs(5);
const UNINSTALL_DEADLINE: Duration = Duration::from_secs(20);

/// Where the kit keeps the copy Herdr links.
pub fn labels_home(home: &Path) -> PathBuf {
    crate::record::kit_state_dir(home)
        .join("plugins")
        .join(PACKAGED)
}

fn packaged(target: &KitTarget) -> PathBuf {
    target.kit_dir.join(PACKAGED)
}

fn request(target: &KitTarget, method: &str, params: Value) -> Result<Value, String> {
    hide_herdr_client::request_with_timeout(&target.herdr_socket, method, params, HERDR_DEADLINE)
        .map_err(|error| format!("Herdr on this machine did not answer {method}: {error}"))
}

fn linked(target: &KitTarget) -> Result<Option<InstalledPluginInfo>, String> {
    let result = request(
        target,
        "plugin.list",
        json!({ "plugin_id": LABELS_PLUGIN_ID }),
    )?;
    let plugins = result
        .get("plugins")
        .cloned()
        .ok_or_else(|| "Herdr's plugin list had no plugins".to_owned())?;
    let plugins: Vec<InstalledPluginInfo> = serde_json::from_value(plugins)
        .map_err(|error| format!("Herdr's plugin list could not be read: {error}"))?;
    Ok(plugins
        .into_iter()
        .find(|plugin| plugin.plugin_id == LABELS_PLUGIN_ID))
}

/// The copy's path as Herdr records it: resolved, so a `~/.hide` reached
/// through a link still compares equal.
fn canonical_home(target: &KitTarget) -> PathBuf {
    let home = labels_home(&target.home);
    std::fs::canonicalize(&home).unwrap_or(home)
}

fn is_ours(target: &KitTarget, plugin: &InstalledPluginInfo) -> bool {
    matches!(plugin.source.kind, PluginSourceKind::Local)
        && Path::new(&plugin.plugin_root) == canonical_home(target)
}

pub(crate) fn observe(target: &KitTarget) -> Observed {
    if !packaged(target).join("herdr-plugin.toml").is_file() {
        return Observed::Blocked(format!(
            "this build has no labels plugin at {}",
            packaged(target).display()
        ));
    }
    let plugin = match linked(target) {
        Ok(plugin) => plugin,
        Err(reason) => return Observed::Blocked(reason),
    };
    let Some(plugin) = plugin else {
        return Observed::Missing;
    };
    if !is_ours(target, &plugin) {
        let from = match plugin.source.kind {
            PluginSourceKind::Github => "GitHub".to_owned(),
            PluginSourceKind::Local => plugin.plugin_root.clone(),
        };
        return Observed::Stale(format!("the plugin is installed from {from}"));
    }
    match payload::is_current(&packaged(target), &labels_home(&target.home)) {
        Ok(true) => Observed::Current,
        Ok(false) => Observed::Stale("an older copy of the plugin is linked".to_owned()),
        Err(reason) => Observed::Blocked(reason),
    }
}

pub(crate) fn install(target: &KitTarget) -> Result<(), String> {
    payload::sync(&packaged(target), &labels_home(&target.home))?;
    let existing = linked(target)?;
    // An operator who turned the plugin off in Herdr keeps it off.
    let enabled = existing.as_ref().is_none_or(|plugin| plugin.enabled);
    if let Some(plugin) = existing.as_ref().filter(|plugin| !is_ours(target, plugin)) {
        match plugin.source.kind {
            PluginSourceKind::Github => uninstall_managed(target)?,
            PluginSourceKind::Local => {
                request(
                    target,
                    "plugin.unlink",
                    json!({ "plugin_id": LABELS_PLUGIN_ID }),
                )?;
            }
        }
    }
    request(
        target,
        "plugin.link",
        json!({
            "path": labels_home(&target.home).display().to_string(),
            "enabled": enabled,
        }),
    )?;
    Ok(())
}

/// A GitHub install is Herdr's managed checkout, which only the `herdr` CLI
/// takes out; unlinking it through the socket would leave the checkout.
fn uninstall_managed(target: &KitTarget) -> Result<(), String> {
    let herdr = target.herdr_bin.as_deref().ok_or_else(|| {
        "the plugin is installed from GitHub and no herdr command was found to replace it"
            .to_owned()
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
        &["plugin", "uninstall", LABELS_PLUGIN_ID],
        &env,
        &target.home,
        UNINSTALL_DEADLINE,
        &target.stop,
    )?;
    if finished.succeeded() {
        Ok(())
    } else {
        Err(format!(
            "herdr could not remove the GitHub copy of the plugin: {}",
            finished.last_error_line()
        ))
    }
}

pub(crate) fn remove(target: &KitTarget) -> RemoveOutcome {
    let plugin = match linked(target) {
        Ok(plugin) => plugin,
        Err(reason) => return RemoveOutcome::Failed { reason },
    };
    let outcome = match plugin {
        Some(plugin) if is_ours(target, &plugin) => {
            match request(
                target,
                "plugin.unlink",
                json!({ "plugin_id": LABELS_PLUGIN_ID }),
            ) {
                Ok(_) => RemoveOutcome::Removed,
                Err(reason) => return RemoveOutcome::Failed { reason },
            }
        }
        Some(_) => RemoveOutcome::Kept {
            reason: "the plugin there was not installed by Hide".to_owned(),
        },
        None => RemoveOutcome::Absent,
    };
    let _ = std::fs::remove_dir_all(labels_home(&target.home));
    outcome
}
