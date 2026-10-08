//! Hide's OpenCode plugin: the one file of Hide's in OpenCode's global
//! configuration, `~/.config/opencode/plugins/hide.js`.
//!
//! OpenCode has no command hook. Its extension point is a plugin, a module
//! OpenCode loads from `plugins/` at startup and runs inside its own process,
//! so Hide's instrumentation for it is that module: it asks this crate's helper
//! (`hide-agent-hooks opencode <operation>`, `src/opencode/helper.rs`) on each root prompt,
//! each shell or `question` tool call and each change of a subagent's state,
//! and does nothing outside a Herdr pane or once the helper is gone.
//!
//! The file is Hide's alone, so ownership is the file's first line:
//! `// hide-opencode-plugin@<version> sha256=<hash of the rest>`. A file
//! without it is someone else's and is never touched; a file whose rest no
//! longer hashes to it was edited by the operator, and stays until Reinstall.
//! Hide never creates OpenCode's configuration folder: `plugins/` is made
//! only inside one OpenCode (or the operator) already made.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The marker's name on the plugin's first line.
pub const PLUGIN_SOURCE_NAME: &str = "hide-opencode-plugin";

/// Raise it when the plugin's text changes.
pub const PLUGIN_VERSION: u32 = 1;

/// The file's name inside OpenCode's `plugins/` folder.
pub const PLUGIN_FILE_NAME: &str = "hide.js";

const TEMPLATE: &str = include_str!("opencode/plugin.js");

/// The one placeholder the template carries: the helper's path, written as a
/// JavaScript string.
const HELPER_PLACEHOLDER: &str = "__HIDE_HELPER__";

/// The line of the template that names the helper.
const HELPER_LINE_START: &str = "const HELPER = ";

/// OpenCode's global configuration folder as Herdr's integration finds it,
/// `~/.config/opencode` on every system, so Hide's plugin sits beside Herdr's.
/// OpenCode itself reads `$XDG_CONFIG_HOME/opencode` when that is set; such a
/// machine loads neither plugin from here (known limitation, shared with Herdr).
pub fn config_directory(home: &Path) -> PathBuf {
    home.join(".config").join("opencode")
}

pub fn plugin_path(home: &Path) -> PathBuf {
    config_directory(home)
        .join("plugins")
        .join(PLUGIN_FILE_NAME)
}

/// Whether Hide writes the plugin on this system. The plugin starts the helper
/// by its path with no shell, which has been checked on macOS and Linux only.
pub fn supported_here() -> Result<(), &'static str> {
    if cfg!(windows) {
        return Err(
            "starting Hide's helper from an OpenCode plugin has not been checked on Windows",
        );
    }
    Ok(())
}

fn body(helper: &Path) -> String {
    // A JSON string is a JavaScript string literal, whatever the path holds.
    let literal =
        serde_json::to_string(&helper.display().to_string()).expect("a string always serializes");
    TEMPLATE.replacen(HELPER_PLACEHOLDER, &literal, 1)
}

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The exact bytes Hide writes for `helper`.
pub fn plugin_text(helper: &Path) -> String {
    let body = body(helper);
    format!(
        "// {PLUGIN_SOURCE_NAME}@{PLUGIN_VERSION} sha256={}\n{body}",
        digest(&body)
    )
}

/// The version and hash a first line states, when it is Hide's marker.
fn marker(line: &str) -> Option<(u32, &str)> {
    let rest = line.strip_prefix("// ")?.strip_prefix(PLUGIN_SOURCE_NAME)?;
    let (version, hash) = rest.strip_prefix('@')?.split_once(" sha256=")?;
    let valid = hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit());
    valid.then_some((version.parse().ok()?, hash))
}

/// The helper path the plugin's body names.
fn helper_of(body: &str) -> Option<String> {
    let literal = body
        .lines()
        .find_map(|line| line.strip_prefix(HELPER_LINE_START))?
        .strip_suffix(';')?;
    serde_json::from_str(literal).ok()
}

/// What is at the plugin's path, before the kit decides anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PluginObserved {
    /// OpenCode has not made its configuration folder, so there is nowhere
    /// to put the plugin yet.
    ConfigAbsent,
    Missing,
    /// This build's plugin for this helper.
    Current,
    /// Hide's plugin, unedited, but not this build's; the sentence says what
    /// is there. A pass replaces it.
    Stale(String),
    /// Hide's marker over a body that no longer hashes to it: the operator
    /// edited it. A pass leaves it and only Reinstall puts Hide's back.
    Edited,
    /// A file there that Hide did not write; it is left as it is.
    Foreign,
    Unreadable(String),
}

/// Judges the plugin file against the one `helper` would be named in.
pub fn observe(home: &Path, helper: &Path) -> PluginObserved {
    if !config_directory(home).is_dir() {
        return PluginObserved::ConfigAbsent;
    }
    let path = plugin_path(home);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return PluginObserved::Missing,
        Err(error) => {
            return PluginObserved::Unreadable(format!(
                "{} could not be read: {error}",
                path.display()
            ));
        }
    };
    let (first, body) = text.split_once('\n').unwrap_or((text.as_str(), ""));
    let Some((version, hash)) = marker(first) else {
        return PluginObserved::Foreign;
    };
    if !digest(body).eq_ignore_ascii_case(hash) {
        return PluginObserved::Edited;
    }
    if text == plugin_text(helper) {
        return PluginObserved::Current;
    }
    let found = helper_of(body);
    PluginObserved::Stale(match found {
        Some(found) if !Path::new(&found).exists() => {
            format!("the plugin points at {found}, which is gone")
        }
        _ if version != PLUGIN_VERSION => format!(
            "version {version} of the plugin is there; this build writes version {PLUGIN_VERSION}"
        ),
        Some(found) if Path::new(&found) != helper => format!(
            "the plugin runs {found}, not this build's {}",
            helper.display()
        ),
        _ => "the plugin is not this build's".to_owned(),
    })
}

/// Writes this build's plugin. A file Hide did not write is never replaced, and
/// OpenCode's configuration folder is never created. Writing the same bytes
/// twice writes nothing; true when the file changed.
pub fn install(home: &Path, helper: &Path) -> Result<bool, String> {
    supported_here().map_err(str::to_owned)?;
    let path = plugin_path(home);
    match observe(home, helper) {
        PluginObserved::Current => return Ok(false),
        PluginObserved::ConfigAbsent => {
            return Err(format!(
                "{} does not exist; the plugin is put in once OpenCode has made it",
                config_directory(home).display()
            ));
        }
        PluginObserved::Foreign => {
            return Err(format!(
                "{} is a file Hide did not write; Hide left it alone",
                path.display()
            ));
        }
        PluginObserved::Unreadable(reason) => return Err(reason),
        PluginObserved::Missing | PluginObserved::Stale(_) | PluginObserved::Edited => {}
    }
    let folder = path.parent().expect("the plugin path has a folder");
    match std::fs::create_dir(folder) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("{} could not be made: {error}", folder.display())),
    }
    hide_platform::fs::atomic::write_file(
        &path,
        plugin_text(helper).as_bytes(),
        hide_platform::fs::Access::KeepOrPrivate,
    )
    .map(|_| true)
    .map_err(|error| format!("{} could not be written: {error}", path.display()))
}

/// Takes Hide's unedited plugin out; an edited one carries the operator's
/// changes and stays, as does a file Hide did not write. True when a file went.
pub fn remove(home: &Path) -> Result<bool, String> {
    let path = plugin_path(home);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("{} could not be read: {error}", path.display())),
    };
    let (first, body) = text.split_once('\n').unwrap_or((text.as_str(), ""));
    match marker(first) {
        Some((_, hash)) if digest(body).eq_ignore_ascii_case(hash) => {}
        _ => return Ok(false),
    }
    std::fs::remove_file(&path)
        .map(|()| true)
        .map_err(|error| format!("{} could not be removed: {error}", path.display()))
}

#[cfg(test)]
mod tests;
