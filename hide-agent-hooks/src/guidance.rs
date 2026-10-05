//! The SessionStart guidance hook of the agents beyond Claude Code and Codex
//! (issue #517).
//!
//! Claude Code and Codex are instrumented: their hooks count subagents,
//! read Project Memory and pull letters (`install`, `runtime`). Every other
//! agent whose official documentation describes a command hook gets one
//! thing: at session start the hook prints Hide's guidance, in the field
//! that agent's documentation says it reads as context. It counts nothing,
//! reads no transcript and writes no pane metadata, so it is a separate
//! module with its own marker rather than more variants of
//! [`crate::AgentRuntime`], whose every other match would then need an arm
//! for agents that never feed it (engineering rule 5, 13).
//!
//! The rules are the ones `install` states for the other hooks: a file that
//! does not parse is never written, what another tool wrote is never
//! touched, a removal takes only the entries carrying Hide's marker,
//! and installing twice converges. Each agent's file and entry shape is
//! pinned to its documentation's own example by a test.
//!
//! | Agent | File | Shape |
//! | --- | --- | --- |
//! | Gemini CLI | `~/.gemini/settings.json` | `hooks.SessionStart[{matcher, hooks[{name, type, command, timeout ms}]}]` |
//! | Qwen Code | `~/.qwen/settings.json` | `hooks.SessionStart[{hooks[{name, type, command, timeout s}]}]` |
//! | Factory Droid | `~/.factory/hooks.json`, else the `hooks` key of `settings.json` | `SessionStart[{hooks[{type, command, timeout s}]}]` |
//! | Copilot CLI | `~/.copilot/hooks/hide-guidance.json`, Hide's own | `{version 1, hooks.sessionStart[{type, bash, powershell, timeoutSec}]}` |
//! | Kiro CLI 3.0 | `~/.kiro/hooks/hide-guidance.json`, Hide's own | `{version "v1", hooks[{name, trigger, action{type, command}, timeout s}]}` |

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::install::{
    HookStatus, InstallFailure, InstallOutcome, Quoting, RemoveOutcome, read_document,
    write_document,
};
use crate::runtime::{PURPOSE_CONTEXT, marker_version_of};

/// The marker Hide stamps into every guidance hook it installs, carried as
/// the `--source` argument of the command like the other hooks' marker.
pub const GUIDANCE_SOURCE_NAME: &str = "hide-guidance";

/// Raise it when the command or entry Hide writes changes shape.
pub const GUIDANCE_VERSION: u32 = 1;

/// The one line every guidance hook adds after the shared instruction, so an
/// agent that Hide cannot place in a Workspace still learns where the usage
/// lives. The text itself is read from the binary, not written here.
pub const GUIDANCE_LINE: &str = "Hide is installed on this machine: run `hide browser help` to learn how to read and drive a browser display from here, and `hide workspace info` to see what this checkout's Workspace offers.";

/// How long the agent waits for the hook, in seconds.
const TIMEOUT_SECONDS: u32 = 8;

/// An agent whose command hook Hide writes the guidance into.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GuidanceAgent {
    Gemini,
    Qwen,
    Droid,
    Copilot,
    Kiro,
}

impl GuidanceAgent {
    pub const ALL: [GuidanceAgent; 5] = [
        Self::Gemini,
        Self::Qwen,
        Self::Droid,
        Self::Copilot,
        Self::Kiro,
    ];

    /// The stable identifier: the agent's adapter id, and the `--runtime` of
    /// its hook command.
    pub fn id(self) -> &'static str {
        match self {
            Self::Gemini => "gemini-cli",
            Self::Qwen => "qwen-code",
            Self::Droid => "factory-droid",
            Self::Copilot => "copilot-cli",
            Self::Kiro => "kiro",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|agent| agent.id() == id)
    }

    /// The folder whose presence means the agent is set up for this account.
    pub fn home_directory(self, home: &Path) -> PathBuf {
        match self {
            Self::Gemini => home.join(".gemini"),
            Self::Qwen => home.join(".qwen"),
            Self::Droid => home.join(".factory"),
            Self::Copilot => home.join(".copilot"),
            Self::Kiro => home.join(".kiro"),
        }
    }

    /// Whether Hide writes this agent's hook on this system. The hooks of
    /// Gemini CLI, Qwen Code, Factory Droid and Kiro run a single command
    /// string through a shell their documentation does not name on Windows,
    /// so a guard written for POSIX or PowerShell would be a guess there.
    pub fn supported_here(self) -> Result<(), &'static str> {
        if cfg!(windows) && self != Self::Copilot {
            return Err("its documentation does not say which shell runs a hook on Windows");
        }
        Ok(())
    }

    /// The file Hide writes (and reads back) for this agent.
    pub fn config_path(self, home: &Path) -> PathBuf {
        self.layout(home).path
    }

    fn layout(self, home: &Path) -> Layout {
        let folder = self.home_directory(home);
        match self {
            Self::Gemini => Layout::event_map(folder.join("settings.json"), true, "SessionStart"),
            Self::Qwen => Layout::event_map(folder.join("settings.json"), true, "SessionStart"),
            Self::Droid => {
                // Droid reads `hooks.json` when it exists and the `hooks` key
                // of `settings.json` only when it does not, so creating the
                // file beside a settings file that has hooks would hide them.
                let hooks_file = folder.join("hooks.json");
                let settings = folder.join("settings.json");
                if !hooks_file.exists() && settings_has_hooks(&settings) {
                    Layout::event_map(settings, true, "SessionStart")
                } else {
                    Layout::event_map(hooks_file, false, "SessionStart")
                }
            }
            Self::Copilot => Layout {
                path: folder.join("hooks").join("hide-guidance.json"),
                shape: Shape::Copilot,
                own_file: true,
            },
            Self::Kiro => Layout {
                path: folder.join("hooks").join("hide-guidance.json"),
                shape: Shape::Kiro,
                own_file: true,
            },
        }
    }
}

fn settings_has_hooks(settings: &Path) -> bool {
    matches!(
        read_document(settings),
        Ok(Some(Value::Object(document))) if document.contains_key("hooks")
    )
}

/// Where in the file Hide's entry lives.
#[derive(Clone, Debug)]
struct Layout {
    path: PathBuf,
    shape: Shape,
    /// The file may be Hide's alone: Hide creates it, and deletes it when
    /// nothing else is left in it.
    own_file: bool,
}

#[derive(Clone, Debug)]
enum Shape {
    /// `{"hooks": {"SessionStart": [group]}}` (`under_hooks_key`), or the
    /// events at the top of the file.
    EventMap {
        under_hooks_key: bool,
        event: &'static str,
    },
    /// `{"version": 1, "hooks": {"sessionStart": [entry]}}`.
    Copilot,
    /// `{"version": "v1", "hooks": [entry]}`.
    Kiro,
}

impl Layout {
    fn event_map(path: PathBuf, under_hooks_key: bool, event: &'static str) -> Self {
        Self {
            path,
            shape: Shape::EventMap {
                under_hooks_key,
                event,
            },
            // A file of events at its top level is one Hide may have created
            // (Droid's `hooks.json`), and an emptied one is deleted: left as
            // `{}` it would still shadow the hooks the operator keeps in
            // `settings.json`.
            own_file: !under_hooks_key,
        }
    }
}

// --- What the hook prints --------------------------------------------------------

/// The context a guidance hook delivers: the shared instruction, the
/// pointer at `hide browser help`, and the Workspace guidance when the
/// daemon answered for this checkout.
pub fn session_context(workspace: Option<&str>) -> String {
    match workspace.filter(|context| !context.is_empty()) {
        Some(workspace) => format!("{PURPOSE_CONTEXT}\n\n{GUIDANCE_LINE}\n\n{workspace}"),
        None => format!("{PURPOSE_CONTEXT}\n\n{GUIDANCE_LINE}"),
    }
}

/// What the agent's hook writes to stdout for `context`, in the form its
/// documentation says it reads: Gemini CLI and Copilot CLI parse exactly one
/// JSON value, Qwen Code and Factory Droid read the JSON envelope, and Kiro
/// reads plain text.
pub fn stdout(agent: GuidanceAgent, context: &str) -> String {
    match agent {
        GuidanceAgent::Gemini => {
            json!({ "hookSpecificOutput": { "additionalContext": context } }).to_string()
        }
        GuidanceAgent::Qwen | GuidanceAgent::Droid => json!({
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": context,
            }
        })
        .to_string(),
        GuidanceAgent::Copilot => json!({ "additionalContext": context }).to_string(),
        GuidanceAgent::Kiro => context.to_owned(),
    }
}

// --- Writing the entry -----------------------------------------------------------

fn arguments(agent: GuidanceAgent) -> String {
    format!(
        "hook --runtime {} --event SessionStart --source {GUIDANCE_SOURCE_NAME}@{GUIDANCE_VERSION}",
        agent.id()
    )
}

/// The guarded POSIX command: it runs the helper only while it is there, so
/// a removed app or helper folder is a hook that does nothing and succeeds
/// (`docs/agent-hooks.md`, Installing).
fn posix_command(helper: &Path, agent: GuidanceAgent) -> String {
    let quoted = Quoting::Posix.quote(&helper.display().to_string());
    format!(
        "if [ -x {quoted} ]; then exec {quoted} {}; fi",
        arguments(agent)
    )
}

/// The same guard for PowerShell, the one Windows form a documented key
/// exists for (Copilot CLI's `powershell`).
fn powershell_command(helper: &Path, agent: GuidanceAgent) -> String {
    let quoted = Quoting::PowerShell.quote(&helper.display().to_string());
    format!(
        "if (Test-Path -LiteralPath {quoted} -PathType Leaf) {{ & {quoted} {} }}",
        arguments(agent)
    )
}

fn entry(agent: GuidanceAgent, helper: &Path) -> Value {
    let command = if cfg!(windows) {
        powershell_command(helper, agent)
    } else {
        posix_command(helper, agent)
    };
    match agent {
        GuidanceAgent::Gemini => json!({
            "matcher": "*",
            "hooks": [{
                "name": GUIDANCE_SOURCE_NAME,
                "type": "command",
                "command": command,
                // Gemini CLI counts milliseconds.
                "timeout": TIMEOUT_SECONDS * 1000,
            }],
        }),
        GuidanceAgent::Qwen => json!({
            "hooks": [{
                "name": GUIDANCE_SOURCE_NAME,
                "type": "command",
                "command": command,
                "timeout": TIMEOUT_SECONDS,
            }],
        }),
        GuidanceAgent::Droid => json!({
            "hooks": [{
                "type": "command",
                "command": command,
                "timeout": TIMEOUT_SECONDS,
            }],
        }),
        GuidanceAgent::Copilot => json!({
            "type": "command",
            "bash": posix_command(helper, agent),
            "powershell": powershell_command(helper, agent),
            "timeoutSec": TIMEOUT_SECONDS,
        }),
        GuidanceAgent::Kiro => json!({
            "name": GUIDANCE_SOURCE_NAME,
            "trigger": "SessionStart",
            "action": { "type": "command", "command": command },
            "timeout": TIMEOUT_SECONDS,
        }),
    }
}

// --- Reading and changing the document -----------------------------------------

fn failure_shape(path: &Path, detail: &str) -> InstallFailure {
    InstallFailure::UnexpectedShape {
        path: path.display().to_string(),
        detail: detail.to_owned(),
    }
}

/// The array Hide's entry lives in, created when `create`.
fn entries<'a>(
    document: &'a mut Value,
    layout: &Layout,
    create: bool,
) -> Result<Option<&'a mut Vec<Value>>, InstallFailure> {
    let path = &layout.path;
    let root = document
        .as_object_mut()
        .ok_or_else(|| failure_shape(path, "the document is not a JSON object"))?;
    match &layout.shape {
        Shape::EventMap {
            under_hooks_key,
            event,
        } => {
            let container: &mut Map<String, Value> = if *under_hooks_key {
                let slot = if create {
                    root.entry("hooks")
                        .or_insert_with(|| Value::Object(Map::new()))
                } else {
                    match root.get_mut("hooks") {
                        Some(slot) => slot,
                        None => return Ok(None),
                    }
                };
                slot.as_object_mut()
                    .ok_or_else(|| failure_shape(path, "\"hooks\" is not an object"))?
            } else {
                root
            };
            let slot = if create {
                container
                    .entry(*event)
                    .or_insert_with(|| Value::Array(Vec::new()))
            } else {
                match container.get_mut(*event) {
                    Some(slot) => slot,
                    None => return Ok(None),
                }
            };
            slot.as_array_mut()
                .map(Some)
                .ok_or_else(|| failure_shape(path, &format!("\"{event}\" is not an array")))
        }
        Shape::Copilot => {
            let hooks = if create {
                root.entry("hooks")
                    .or_insert_with(|| Value::Object(Map::new()))
            } else {
                match root.get_mut("hooks") {
                    Some(slot) => slot,
                    None => return Ok(None),
                }
            };
            let hooks = hooks
                .as_object_mut()
                .ok_or_else(|| failure_shape(path, "\"hooks\" is not an object"))?;
            let slot = if create {
                hooks
                    .entry("sessionStart")
                    .or_insert_with(|| Value::Array(Vec::new()))
            } else {
                match hooks.get_mut("sessionStart") {
                    Some(slot) => slot,
                    None => return Ok(None),
                }
            };
            slot.as_array_mut()
                .map(Some)
                .ok_or_else(|| failure_shape(path, "\"hooks.sessionStart\" is not an array"))
        }
        Shape::Kiro => {
            let slot = if create {
                root.entry("hooks")
                    .or_insert_with(|| Value::Array(Vec::new()))
            } else {
                match root.get_mut("hooks") {
                    Some(slot) => slot,
                    None => return Ok(None),
                }
            };
            slot.as_array_mut()
                .map(Some)
                .ok_or_else(|| failure_shape(path, "\"hooks\" is not an array"))
        }
    }
}

/// Every string anywhere in an entry. The marker is looked for in all of
/// them, so ownership never depends on which key an agent keeps its command
/// under.
fn strings<'a>(value: &'a Value, found: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => found.push(text),
        Value::Array(items) => items.iter().for_each(|item| strings(item, found)),
        Value::Object(map) => map.values().for_each(|item| strings(item, found)),
        _ => {}
    }
}

fn entry_marker_version(entry: &Value) -> Option<u32> {
    let mut found = Vec::new();
    strings(entry, &mut found);
    found
        .into_iter()
        .filter_map(|text| marker_version_of(GUIDANCE_SOURCE_NAME, text))
        .min()
}

/// The helper path in the first entry Hide owns.
fn entry_helper(entry: &Value) -> Option<String> {
    let mut found = Vec::new();
    strings(entry, &mut found);
    found
        .into_iter()
        .filter(|text| text.contains(GUIDANCE_SOURCE_NAME) && text.contains('\''))
        .find_map(|text| Quoting::NATIVE.first_quoted(text))
}

/// Takes Hide's own hook out of `list` and nothing else. A group another
/// tool shares with Hide (`{"matcher": "*", "hooks": [hide, theirs]}`) keeps
/// the other tool's hook and loses only Hide's; a group left with no hook
/// goes, and so does a flat entry that carries the marker. Returns how many
/// of Hide's hooks were taken out.
fn strip_owned(list: &mut Vec<Value>) -> usize {
    let mut removed = 0;
    list.retain_mut(|entry| {
        if let Some(hooks) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
            let before = hooks.len();
            hooks.retain(|hook| entry_marker_version(hook).is_none());
            let taken = before - hooks.len();
            removed += taken;
            return taken == 0 || !hooks.is_empty();
        }
        if entry_marker_version(entry).is_some() {
            removed += 1;
            return false;
        }
        true
    });
    removed
}

fn owned(entries: &[Value]) -> impl Iterator<Item = &Value> {
    entries
        .iter()
        .filter(|entry| entry_marker_version(entry).is_some())
}

/// Judges one agent without changing anything.
pub fn status(agent: GuidanceAgent, home: &Path) -> HookStatus {
    if !agent.home_directory(home).is_dir() {
        return HookStatus::RuntimeAbsent;
    }
    let layout = agent.layout(home);
    let mut document = match read_document(&layout.path) {
        Ok(Some(document)) => document,
        Ok(None) => return HookStatus::NotInstalled,
        Err(reason) => return HookStatus::Failed { reason },
    };
    let found = match entries(&mut document, &layout, false) {
        Ok(Some(found)) => found,
        Ok(None) => return HookStatus::NotInstalled,
        Err(reason) => return HookStatus::Failed { reason },
    };
    let Some(version) = owned(found).filter_map(entry_marker_version).min() else {
        return HookStatus::NotInstalled;
    };
    if let Some(helper) = owned(found).find_map(entry_helper)
        && !Path::new(&helper).exists()
    {
        return HookStatus::Failed {
            reason: InstallFailure::HelperMissing {
                path: layout.path.display().to_string(),
                helper,
            },
        };
    }
    if version < GUIDANCE_VERSION {
        HookStatus::Outdated { version }
    } else {
        HookStatus::Installed { version }
    }
}

/// The helper Hide's entry in `agent`'s file names, when there is one; the
/// kit compares it with the helper it would install.
pub fn installed_helper_path(agent: GuidanceAgent, home: &Path) -> Option<String> {
    let layout = agent.layout(home);
    let mut document = read_document(&layout.path).ok()??;
    let found = entries(&mut document, &layout, false).ok()??;
    owned(found).find_map(entry_helper)
}

fn blank_document(layout: &Layout) -> Value {
    match layout.shape {
        Shape::EventMap { .. } => Value::Object(Map::new()),
        Shape::Copilot => json!({ "version": 1 }),
        Shape::Kiro => json!({ "version": "v1" }),
    }
}

/// Appends Hide's entry, preserving everything already in the file; a
/// second install converges on one entry.
pub fn install(
    agent: GuidanceAgent,
    home: &Path,
    helper: &Path,
) -> Result<InstallOutcome, InstallFailure> {
    agent
        .supported_here()
        .map_err(|why| InstallFailure::UnexpectedShape {
            path: agent.config_path(home).display().to_string(),
            detail: why.to_owned(),
        })?;
    let layout = agent.layout(home);
    let mut document = read_document(&layout.path)?.unwrap_or_else(|| blank_document(&layout));
    let before = serde_json::to_string(&document).unwrap_or_default();
    let list = entries(&mut document, &layout, true)?
        .ok_or_else(|| failure_shape(&layout.path, "the hook list could not be created"))?;
    strip_owned(list);
    let preserved = list.len();
    list.push(entry(agent, helper));
    let after = serde_json::to_string(&document).unwrap_or_default();
    if before == after {
        return Ok(InstallOutcome {
            changed: false,
            preserved_entries: preserved,
        });
    }
    write_document(&layout.path, &document)?;
    Ok(InstallOutcome {
        changed: true,
        preserved_entries: preserved,
    })
}

/// Takes out only the entries carrying Hide's marker; a file that was Hide's
/// alone goes with them.
pub fn remove(agent: GuidanceAgent, home: &Path) -> Result<RemoveOutcome, InstallFailure> {
    // Every file Hide could have written to, so an entry written beside a
    // settings file is still found after a `hooks.json` appeared (Droid).
    let mut layouts = vec![agent.layout(home)];
    if agent == GuidanceAgent::Droid {
        let folder = agent.home_directory(home);
        for layout in [
            Layout::event_map(folder.join("hooks.json"), false, "SessionStart"),
            Layout::event_map(folder.join("settings.json"), true, "SessionStart"),
        ] {
            if !layouts.iter().any(|known| known.path == layout.path) {
                layouts.push(layout);
            }
        }
    }
    let mut total = RemoveOutcome {
        changed: false,
        removed_entries: 0,
        preserved_entries: 0,
    };
    for layout in &layouts {
        let outcome = remove_in(layout)?;
        total.changed |= outcome.changed;
        total.removed_entries += outcome.removed_entries;
        total.preserved_entries += outcome.preserved_entries;
    }
    Ok(total)
}

fn remove_in(layout: &Layout) -> Result<RemoveOutcome, InstallFailure> {
    let Some(mut document) = read_document(&layout.path)? else {
        return Ok(RemoveOutcome {
            changed: false,
            removed_entries: 0,
            preserved_entries: 0,
        });
    };
    let Some(list) = entries(&mut document, layout, false)? else {
        return Ok(RemoveOutcome {
            changed: false,
            removed_entries: 0,
            preserved_entries: 0,
        });
    };
    let removed = strip_owned(list);
    let preserved = list.len();
    if removed == 0 {
        return Ok(RemoveOutcome {
            changed: false,
            removed_entries: 0,
            preserved_entries: preserved,
        });
    }
    if preserved == 0 {
        drop_empty(&mut document, layout);
    }
    if layout.own_file && only_scaffolding(&document, layout) {
        std::fs::remove_file(&layout.path).map_err(|error| InstallFailure::NotWritable {
            path: layout.path.display().to_string(),
            detail: error.to_string(),
        })?;
    } else {
        write_document(&layout.path, &document)?;
    }
    Ok(RemoveOutcome {
        changed: true,
        removed_entries: removed,
        preserved_entries: preserved,
    })
}

/// Drops the event key Hide emptied, so the operator's file does not keep an
/// empty list Hide made.
fn drop_empty(document: &mut Value, layout: &Layout) {
    let Some(root) = document.as_object_mut() else {
        return;
    };
    match &layout.shape {
        Shape::EventMap {
            under_hooks_key,
            event,
        } => {
            if *under_hooks_key {
                if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
                    hooks.remove(*event);
                }
            } else {
                root.remove(*event);
            }
        }
        Shape::Copilot => {
            if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
                hooks.remove("sessionStart");
            }
        }
        Shape::Kiro => {
            root.remove("hooks");
        }
    }
}

/// Whether nothing but the scaffolding Hide wrote is left in an own file.
fn only_scaffolding(document: &Value, layout: &Layout) -> bool {
    let Some(root) = document.as_object() else {
        return false;
    };
    let hooks_empty = match &layout.shape {
        Shape::Copilot => root
            .get("hooks")
            .and_then(Value::as_object)
            .is_none_or(Map::is_empty),
        Shape::Kiro => !root.contains_key("hooks"),
        Shape::EventMap { .. } => root.is_empty(),
    };
    hooks_empty && root.keys().all(|key| key == "version" || key == "hooks")
}

#[cfg(test)]
mod tests;
