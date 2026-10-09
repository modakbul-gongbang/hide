//! A script file Hide owns in an agent's own extension folder: OpenCode's
//! plugin (`crate::opencode`), and the one extension Pi and omp share
//! (`crate::pi_extension`).
//!
//! These agents have no command hook. Their extension point is a module the
//! agent loads from a folder at startup and runs inside its own process, so
//! Hide's instrumentation for them is that module: it asks this crate's
//! helper (`hide-agent-hooks <agent> <operation>`, `src/plugin/helper.rs`)
//! and does nothing outside a Herdr pane or once the helper is gone.
//!
//! The file is Hide's alone, so ownership is the file's first line:
//! `// <marker>@<version> sha256=<hash of the rest>`. A file without it is
//! someone else's and is never touched; a file whose rest no longer hashes to
//! it was edited by the operator, and stays until Reinstall. Hide never
//! creates the agent's own folder: the extension folder is made only inside
//! one the agent (or the operator) already made.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use hide_agent_adapter::PluginDialect;
use sha2::{Digest, Sha256};

/// The one placeholder every template carries: the helper's path, written as
/// a JavaScript string.
const HELPER_PLACEHOLDER: &str = "__HIDE_HELPER__";

/// The line of a template that names the helper.
const HELPER_LINE_START: &str = "const HELPER = ";

/// One agent's script file: where it goes, the marker that proves it is
/// Hide's, and the template it is made from.
#[derive(Debug)]
pub struct PluginFile {
    /// The agent's name in a sentence.
    pub agent: &'static str,
    /// What the agent calls the file: a plugin or an extension.
    pub noun: &'static str,
    /// The marker's name on the file's first line.
    pub source_name: &'static str,
    /// Raised when the file's text changes.
    pub version: u32,
    /// The agent's own folder below the home, which Hide never creates.
    pub config: &'static [&'static str],
    /// The folder inside it the agent loads scripts from, which Hide may create.
    pub folder: &'static str,
    /// The file's name inside that folder.
    pub file_name: &'static str,
    /// The script, with `__HIDE_HELPER__` where the helper's path goes.
    pub template: &'static str,
    /// Further placeholders and the JavaScript each one becomes.
    pub constants: &'static [(&'static str, &'static str)],
}

/// The file `dialect` names.
pub fn file(dialect: PluginDialect) -> &'static PluginFile {
    match dialect {
        PluginDialect::OpenCode => &crate::opencode::PLUGIN,
        PluginDialect::Pi => &crate::pi_extension::PI,
        PluginDialect::Omp => &crate::pi_extension::OMP,
    }
}

/// Whether Hide writes these files on this system. The script starts the
/// helper by its path with no shell, which has been checked on macOS and
/// Linux only.
pub fn supported_here() -> Result<(), &'static str> {
    if cfg!(windows) {
        return Err(
            "starting Hide's helper from an agent's extension has not been checked on Windows",
        );
    }
    Ok(())
}

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// What is at the file's path, before the kit decides anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PluginObserved {
    /// The agent has not made its own folder, so there is nowhere to put the
    /// file yet.
    ConfigAbsent,
    Missing,
    /// This build's file for this helper.
    Current,
    /// Hide's file, unedited, but not this build's; the sentence says what is
    /// there. A pass replaces it.
    Stale(String),
    /// Hide's marker over a body that no longer hashes to it: the operator
    /// edited it. A pass leaves it and only Reinstall puts Hide's back.
    Edited,
    /// A file there that Hide did not write; it is left as it is.
    Foreign,
    Unreadable(String),
}

impl PluginFile {
    /// The agent's own folder, the one Herdr's integration for it uses too.
    pub fn config_directory(&self, home: &Path) -> PathBuf {
        self.config
            .iter()
            .fold(home.to_path_buf(), |path, part| path.join(part))
    }

    pub fn path(&self, home: &Path) -> PathBuf {
        self.config_directory(home)
            .join(self.folder)
            .join(self.file_name)
    }

    fn body(&self, helper: &Path) -> String {
        // A JSON string is a JavaScript string literal, whatever the path holds.
        let literal = serde_json::to_string(&helper.display().to_string())
            .expect("a string always serializes");
        // The constants go in first, so a placeholder's spelling inside the
        // helper's path is left as the path's.
        self.constants
            .iter()
            .fold(self.template.to_owned(), |body, (placeholder, value)| {
                body.replacen(placeholder, value, 1)
            })
            .replacen(HELPER_PLACEHOLDER, &literal, 1)
    }

    /// The exact bytes Hide writes for `helper`.
    pub fn text(&self, helper: &Path) -> String {
        let body = self.body(helper);
        format!(
            "// {}@{} sha256={}\n{body}",
            self.source_name,
            self.version,
            digest(&body)
        )
    }

    /// The version and hash a first line states, when it is this file's marker.
    fn marker<'a>(&self, line: &'a str) -> Option<(u32, &'a str)> {
        let rest = line.strip_prefix("// ")?.strip_prefix(self.source_name)?;
        let (version, hash) = rest.strip_prefix('@')?.split_once(" sha256=")?;
        let valid = hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit());
        valid.then_some((version.parse().ok()?, hash))
    }

    /// Judges the file against the one `helper` would be named in.
    pub fn observe(&self, home: &Path, helper: &Path) -> PluginObserved {
        if !self.config_directory(home).is_dir() {
            return PluginObserved::ConfigAbsent;
        }
        let path = self.path(home);
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
        let Some((version, hash)) = self.marker(first) else {
            return PluginObserved::Foreign;
        };
        if !digest(body).eq_ignore_ascii_case(hash) {
            return PluginObserved::Edited;
        }
        if text == self.text(helper) {
            return PluginObserved::Current;
        }
        let what = self.noun;
        PluginObserved::Stale(match helper_of(body) {
            Some(found) if !Path::new(&found).exists() => {
                format!("the {what} points at {found}, which is gone")
            }
            _ if version != self.version => format!(
                "version {version} of the {what} is there; this build writes version {}",
                self.version
            ),
            Some(found) if Path::new(&found) != helper => format!(
                "the {what} runs {found}, not this build's {}",
                helper.display()
            ),
            _ => format!("the {what} is not this build's"),
        })
    }

    /// Writes this build's file. A file Hide did not write is never replaced,
    /// and the agent's own folder is never created. Writing the same bytes
    /// twice writes nothing; true when the file changed.
    pub fn install(&self, home: &Path, helper: &Path) -> Result<bool, String> {
        supported_here().map_err(str::to_owned)?;
        let path = self.path(home);
        match self.observe(home, helper) {
            PluginObserved::Current => return Ok(false),
            PluginObserved::ConfigAbsent => {
                return Err(format!(
                    "{} does not exist; the {} is put in once {} has made it",
                    self.config_directory(home).display(),
                    self.noun,
                    self.agent
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
        let folder = path.parent().expect("the file has a folder");
        match std::fs::create_dir(folder) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("{} could not be made: {error}", folder.display())),
        }
        hide_platform::fs::atomic::write_file(
            &path,
            self.text(helper).as_bytes(),
            hide_platform::fs::Access::KeepOrPrivate,
        )
        .map(|_| true)
        .map_err(|error| format!("{} could not be written: {error}", path.display()))
    }

    /// Takes Hide's unedited file out; an edited one carries the operator's
    /// changes and stays, as does a file Hide did not write. True when a file
    /// went.
    pub fn remove(&self, home: &Path) -> Result<bool, String> {
        let path = self.path(home);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(format!("{} could not be read: {error}", path.display())),
        };
        let (first, body) = text.split_once('\n').unwrap_or((text.as_str(), ""));
        match self.marker(first) {
            Some((_, hash)) if digest(body).eq_ignore_ascii_case(hash) => {}
            _ => return Ok(false),
        }
        std::fs::remove_file(&path)
            .map(|()| true)
            .map_err(|error| format!("{} could not be removed: {error}", path.display()))
    }
}

/// The helper path a file's body names.
fn helper_of(body: &str) -> Option<String> {
    let literal = body
        .lines()
        .find_map(|line| line.strip_prefix(HELPER_LINE_START))?
        .strip_suffix(';')?;
    serde_json::from_str(literal).ok()
}

#[cfg(test)]
mod tests;
