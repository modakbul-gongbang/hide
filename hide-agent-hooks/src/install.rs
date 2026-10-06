//! Reading, appending to, and removing from a runtime's global hook file.
//!
//! Every rule here exists because the array Hide appends to is already
//! occupied. On the machine this was designed against, `SubagentStart`
//! already carried two other tools' hooks (PRD D-26), so:
//!
//! - a file that does not parse is a file Hide never writes (PRD D-46);
//! - an append never rewrites an entry it does not own;
//! - a removal takes only the entries carrying Hide's marker (PRD D-57);
//! - installing twice converges instead of duplicating (PRD B25).

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::runtime::{AgentRuntime, HookEvent, hook_source_id, marker_version_in};

/// Why Hide did not change a runtime's hook file.
///
/// Each variant names a cause the operator can act on; none of them is a
/// state Hide papers over by writing anyway.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum InstallFailure {
    /// The file exists but could not be read: permissions, or an I/O error.
    Unreadable { path: String, detail: String },
    /// The file is not valid JSON. Hide cannot append to what it cannot
    /// parse, and rewriting it from scratch would delete another tool's
    /// hooks, so it stops here (PRD D-46).
    Unparsable { path: String, detail: String },
    /// The file parses but `hooks`, or one event under it, is not the shape
    /// the runtime documents.
    UnexpectedShape { path: String, detail: String },
    /// The write itself failed.
    NotWritable { path: String, detail: String },
    /// Hide's entry points at a helper that is no longer on disk, so the hook
    /// runs nothing. Reinstalling from the running app repairs it.
    HelperMissing { path: String, helper: String },
}

impl InstallFailure {
    /// One sentence for the operator, in the Settings diagnosis and the CLI.
    pub fn message(&self) -> String {
        match self {
            Self::Unreadable { path, detail } => format!("{path} could not be read: {detail}"),
            Self::Unparsable { path, detail } => {
                format!("{path} is not valid JSON ({detail}); Hide left it untouched")
            }
            Self::UnexpectedShape { path, detail } => {
                format!("{path} has an unexpected shape: {detail}")
            }
            Self::NotWritable { path, detail } => format!("{path} could not be written: {detail}"),
            Self::HelperMissing { helper, .. } => {
                format!("the installed hook points at {helper}, which is missing")
            }
        }
    }
}

/// What Hide currently finds in one runtime's hook file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HookStatus {
    /// The runtime is not set up on this Mac, so there is nothing to install.
    RuntimeAbsent,
    /// Hide's entries are present at the current version.
    Installed { version: u32 },
    /// Hide's entries are present at an older version, or are missing from
    /// some of the events Hide registers.
    Outdated { version: u32 },
    /// The runtime is here and carries no entry of Hide's.
    NotInstalled,
    /// The operator switched the agent off in Settings, Agents, so Hide's
    /// hook is meant to be absent. Nothing in the runtime's file says so:
    /// the kit's record does, and whoever holds it sets this.
    Off,
    /// The file could not be read, parsed or written.
    Failed { reason: InstallFailure },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallOutcome {
    /// False when the file already said exactly this; the second install of
    /// the same intent writes nothing (engineering rule 11).
    pub changed: bool,
    /// Entries belonging to other tools that survived the append. The count is
    /// what the regression test asserts.
    pub preserved_entries: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoveOutcome {
    pub changed: bool,
    pub removed_entries: usize,
    pub preserved_entries: usize,
}

/// The `hooks` object every runtime keeps at the top level of its file.
const HOOKS_KEY: &str = "hooks";

/// Judges one runtime without changing anything.
pub fn status(runtime: AgentRuntime, home: &Path) -> HookStatus {
    if !runtime.home_directory(home).is_dir() {
        return HookStatus::RuntimeAbsent;
    }
    let path = runtime.config_path(home);
    let document = match read_document(&path) {
        Ok(Some(document)) => document,
        Ok(None) => return HookStatus::NotInstalled,
        Err(reason) => return HookStatus::Failed { reason },
    };
    let hooks = match hooks_object(&document, &path) {
        Ok(Some(hooks)) => hooks,
        Ok(None) => return HookStatus::NotInstalled,
        Err(reason) => return HookStatus::Failed { reason },
    };
    let mut lowest: Option<u32> = None;
    let mut missing_event = false;
    for event in HookEvent::ALL {
        let version = hooks
            .get(event.name())
            .and_then(Value::as_array)
            .map(|groups| owned_versions(groups))
            .and_then(|versions| versions.into_iter().min());
        match version {
            Some(version) => lowest = Some(lowest.map_or(version, |best| best.min(version))),
            None => missing_event = true,
        }
    }
    let Some(version) = lowest else {
        return HookStatus::NotInstalled;
    };
    if let Some(helper) = installed_helper(hooks)
        && !Path::new(&helper).exists()
    {
        return HookStatus::Failed {
            reason: InstallFailure::HelperMissing {
                path: path.display().to_string(),
                helper,
            },
        };
    }
    if missing_event || version < crate::runtime::HOOK_VERSION {
        HookStatus::Outdated { version }
    } else {
        HookStatus::Installed { version }
    }
}

/// Appends Hide's entries to every event it registers, preserving everything
/// already there.
///
/// `helper` is the absolute path of the `hide-agent-hooks` executable the
/// hook will run. It is written into the command verbatim, so the caller
/// hands the path of the bundle that is actually running.
pub fn install(
    runtime: AgentRuntime,
    home: &Path,
    helper: &Path,
) -> Result<InstallOutcome, InstallFailure> {
    let path = runtime.config_path(home);
    let mut document = read_document(&path)?.unwrap_or_else(|| Value::Object(Map::new()));
    let before = serde_json::to_string(&document).unwrap_or_default();
    let hooks = hooks_object_mut(&mut document, &path)?;
    let mut preserved = 0usize;
    for event in HookEvent::ALL {
        let groups = event_array_mut(hooks, event, &path)?;
        // Remove Hide's own entries first so a second install converges on
        // one entry per event rather than appending another
        // (engineering rule 11, PRD B25).
        groups.retain(|group| group_marker_version(group).is_none());
        preserved += groups.len();
        groups.push(hook_group(helper, runtime, event));
    }
    let after = serde_json::to_string(&document).unwrap_or_default();
    if before == after {
        return Ok(InstallOutcome {
            changed: false,
            preserved_entries: preserved,
        });
    }
    write_document(&path, &document)?;
    Ok(InstallOutcome {
        changed: true,
        preserved_entries: preserved,
    })
}

/// Takes out only the entries carrying Hide's marker.
pub fn remove(runtime: AgentRuntime, home: &Path) -> Result<RemoveOutcome, InstallFailure> {
    let path = runtime.config_path(home);
    let Some(mut document) = read_document(&path)? else {
        return Ok(RemoveOutcome {
            changed: false,
            removed_entries: 0,
            preserved_entries: 0,
        });
    };
    let hooks = hooks_object_mut(&mut document, &path)?;
    let mut removed = 0usize;
    let mut preserved = 0usize;
    let mut emptied = Vec::new();
    for (event, value) in hooks.iter_mut() {
        let Some(groups) = value.as_array_mut() else {
            continue;
        };
        let before = groups.len();
        groups.retain(|group| group_marker_version(group).is_none());
        removed += before - groups.len();
        preserved += groups.len();
        if groups.is_empty() {
            emptied.push(event.clone());
        }
    }
    for event in emptied {
        // Only an event Hide emptied is dropped; an event another tool left
        // empty was already empty before this ran.
        if HookEvent::parse(&event).is_some() {
            hooks.remove(&event);
        }
    }
    if removed == 0 {
        return Ok(RemoveOutcome {
            changed: false,
            removed_entries: 0,
            preserved_entries: preserved,
        });
    }
    write_document(&path, &document)?;
    Ok(RemoveOutcome {
        changed: true,
        removed_entries: removed,
        preserved_entries: preserved,
    })
}

/// The entry Hide writes: one command hook, carrying its own marker, in the
/// form this system's runtimes run a hook command (`docs/agent-hooks.md`,
/// Installing).
///
/// The command runs the helper only while it is there. A hook outlives the
/// bundle or helper folder that wrote it: the app is moved to the Trash, a
/// device is removed while it is offline, a helper folder is cleaned by hand.
/// A bare path would then fail every one of the agent's turns with `No such
/// file or directory`, which is what happened on 2026-09-10; guarded, a
/// missing helper is a hook that does nothing and succeeds (PRD
/// device-parity D-11, B3).
fn hook_group(helper: &Path, runtime: AgentRuntime, event: HookEvent) -> Value {
    let hook = if cfg!(windows) {
        windows_hook(helper, runtime, event)
    } else {
        posix_hook(helper, runtime, event)
    };
    let mut group = serde_json::json!({ "hooks": [hook] });
    if let Some(matcher) = hook_matcher(runtime, event) {
        group["matcher"] = Value::from(matcher);
    }
    group
}

/// The matcher Hide writes on the group of `event`'s entry, `None` for an
/// entry that fires on every occurrence. Both the writer above and the Codex
/// trust step read it here, so an event that gains a matcher (a PreToolUse
/// guard) is written and recognised as Hide's by the same line.
pub(crate) fn hook_matcher(_runtime: AgentRuntime, _event: HookEvent) -> Option<&'static str> {
    None
}

/// The arguments the helper runs with, after its path.
fn helper_arguments(runtime: AgentRuntime, event: HookEvent) -> String {
    format!(
        "hook --runtime {} --event {} --memory-injection --source {}",
        runtime.id(),
        event.name(),
        hook_source_id()
    )
}

/// macOS and Linux: both runtimes hand the command to a POSIX shell (Claude
/// Code `sh -c`, Codex the session's shell, zsh or bash, with `-c`).
fn posix_hook(helper: &Path, runtime: AgentRuntime, event: HookEvent) -> Value {
    serde_json::json!({
        "type": "command",
        "command": posix_command(helper, runtime, event),
        "timeout": 8,
    })
}

fn posix_command(helper: &Path, runtime: AgentRuntime, event: HookEvent) -> String {
    let quoted = Quoting::Posix.quote(&helper.display().to_string());
    format!(
        "if [ -x {quoted} ]; then exec {quoted} {}; fi",
        helper_arguments(runtime, event)
    )
}

/// Windows: one PowerShell guard for both runtimes. Codex runs a command in
/// the session's shell, which on Windows is PowerShell (`-NoProfile
/// -Command`), so the guard is the command. Claude Code runs a command
/// through Git Bash when it is installed and PowerShell otherwise, so its
/// entry names PowerShell itself in exec form (`args`), which no shell
/// re-parses and which runs the same on every machine.
fn windows_hook(helper: &Path, runtime: AgentRuntime, event: HookEvent) -> Value {
    let guard = windows_guard(helper, runtime, event);
    match runtime {
        AgentRuntime::ClaudeCode => serde_json::json!({
            "type": "command",
            "command": "powershell.exe",
            "args": ["-NoProfile", "-NonInteractive", "-Command", guard],
            "timeout": 8,
        }),
        AgentRuntime::Codex => serde_json::json!({
            "type": "command",
            "command": guard,
            "timeout": 8,
        }),
    }
}

fn windows_guard(helper: &Path, runtime: AgentRuntime, event: HookEvent) -> String {
    let quoted = Quoting::PowerShell.quote(&helper.display().to_string());
    format!(
        "if (Test-Path -LiteralPath {quoted} -PathType Leaf) {{ & {quoted} {} }}",
        helper_arguments(runtime, event)
    )
}

/// The `command` of the Codex entry [`install`] writes for `event` on this
/// system. Codex takes the command string itself on every system, so this is
/// the whole of what Codex hashes and shows for the entry; the trust step
/// compares against it instead of rebuilding it (`crate::codex_trust`).
pub(crate) fn codex_command(helper: &Path, event: HookEvent) -> String {
    if cfg!(windows) {
        windows_guard(helper, AgentRuntime::Codex, event)
    } else {
        posix_command(helper, AgentRuntime::Codex, event)
    }
}

/// How a path is quoted in the command this system writes, and read back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Quoting {
    /// Single quotes, a quote inside written `'\''`.
    Posix,
    /// Single quotes, a quote inside doubled. PowerShell takes the
    /// typographic single quotes for `'` as well, so they are doubled too.
    PowerShell,
}

impl Quoting {
    pub(crate) const NATIVE: Self = if cfg!(windows) {
        Self::PowerShell
    } else {
        Self::Posix
    };

    pub(crate) const POWERSHELL_QUOTES: [char; 5] =
        ['\'', '\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}'];

    pub(crate) fn quote(self, value: &str) -> String {
        match self {
            Self::Posix => format!("'{}'", value.replace('\'', "'\\''")),
            Self::PowerShell => {
                let mut quoted = String::with_capacity(value.len() + 2);
                quoted.push('\'');
                for character in value.chars() {
                    if Self::POWERSHELL_QUOTES.contains(&character) {
                        quoted.push(character);
                    }
                    quoted.push(character);
                }
                quoted.push('\'');
                quoted
            }
        }
    }

    /// The first single-quoted word of `command`, unquoted: the helper path,
    /// in the guarded command and in the bare one versions before 6 wrote.
    pub(crate) fn first_quoted(self, command: &str) -> Option<String> {
        let start = command.find('\'')? + 1;
        let mut rest = &command[start..];
        let mut path = String::new();
        match self {
            Self::Posix => loop {
                let end = rest.find('\'')?;
                path.push_str(&rest[..end]);
                rest = &rest[end + 1..];
                match rest.strip_prefix("\\''") {
                    Some(after) => {
                        path.push('\'');
                        rest = after;
                    }
                    None => return Some(path),
                }
            },
            Self::PowerShell => {
                let mut characters = rest.chars().peekable();
                while let Some(character) = characters.next() {
                    if !Self::POWERSHELL_QUOTES.contains(&character) {
                        path.push(character);
                    } else if characters.peek() == Some(&character) {
                        path.push(character);
                        characters.next();
                    } else {
                        return Some(path);
                    }
                }
                None
            }
        }
    }
}

/// The strings of one hook a runtime runs: its `command`, then its exec-form
/// `args`, where Claude Code's Windows entry carries the guard.
fn hook_texts(hook: &Value) -> impl Iterator<Item = &str> {
    let command = hook.get("command").and_then(Value::as_str);
    let args = hook
        .get("args")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);
    command.into_iter().chain(args)
}

fn group_marker_version(group: &Value) -> Option<u32> {
    group
        .get("hooks")?
        .as_array()?
        .iter()
        .flat_map(hook_texts)
        .filter_map(marker_version_in)
        .min()
}

fn owned_versions(groups: &[Value]) -> Vec<u32> {
    groups.iter().filter_map(group_marker_version).collect()
}

/// The helper path recorded in the first entry Hide owns.
fn installed_helper(hooks: &Map<String, Value>) -> Option<String> {
    hooks
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .filter(|group| group_marker_version(group).is_some())
        .filter_map(|group| group.get("hooks")?.as_array()?.first())
        .find_map(|hook| {
            hook_texts(hook)
                .filter(|text| text.contains('\''))
                .find_map(|text| Quoting::NATIVE.first_quoted(text))
        })
}

/// The helper Hide's entries in `runtime`'s file name, when there are any.
/// The kit compares it with the helper it would install, so an entry left by
/// an app at another path is replaced rather than taken as current.
pub fn installed_helper_path(runtime: AgentRuntime, home: &Path) -> Option<String> {
    let document = read_document(&runtime.config_path(home)).ok()??;
    let hooks = hooks_object(&document, &runtime.config_path(home)).ok()??;
    installed_helper(hooks)
}

pub(crate) fn read_document(path: &Path) -> Result<Option<Value>, InstallFailure> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(InstallFailure::Unreadable {
                path: path.display().to_string(),
                detail: error.to_string(),
            });
        }
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|error| InstallFailure::Unparsable {
            path: path.display().to_string(),
            detail: error.to_string(),
        })
}

fn hooks_object<'a>(
    document: &'a Value,
    path: &Path,
) -> Result<Option<&'a Map<String, Value>>, InstallFailure> {
    let Some(object) = document.as_object() else {
        return Err(InstallFailure::UnexpectedShape {
            path: path.display().to_string(),
            detail: "the document is not a JSON object".to_owned(),
        });
    };
    match object.get(HOOKS_KEY) {
        None => Ok(None),
        Some(Value::Object(hooks)) => Ok(Some(hooks)),
        Some(_) => Err(InstallFailure::UnexpectedShape {
            path: path.display().to_string(),
            detail: "\"hooks\" is not an object".to_owned(),
        }),
    }
}

fn hooks_object_mut<'a>(
    document: &'a mut Value,
    path: &Path,
) -> Result<&'a mut Map<String, Value>, InstallFailure> {
    let unexpected = |detail: &str| InstallFailure::UnexpectedShape {
        path: path.display().to_string(),
        detail: detail.to_owned(),
    };
    let object = document
        .as_object_mut()
        .ok_or_else(|| unexpected("the document is not a JSON object"))?;
    let entry = object
        .entry(HOOKS_KEY.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    entry
        .as_object_mut()
        .ok_or_else(|| unexpected("\"hooks\" is not an object"))
}

fn event_array_mut<'a>(
    hooks: &'a mut Map<String, Value>,
    event: HookEvent,
    path: &Path,
) -> Result<&'a mut Vec<Value>, InstallFailure> {
    let entry = hooks
        .entry(event.name().to_owned())
        .or_insert_with(|| Value::Array(Vec::new()));
    entry
        .as_array_mut()
        .ok_or_else(|| InstallFailure::UnexpectedShape {
            path: path.display().to_string(),
            detail: format!("\"hooks.{}\" is not an array", event.name()),
        })
}

/// Writes through a temporary file beside the file it replaces, so a failure
/// part way through leaves the operator's original file intact. The file
/// keeps its permission bits, because a settings file kept at 0600 can hold
/// tokens, and a new one is 0600. A settings file that is a link, as a
/// dotfile manager makes it, is written where the link leads, so the link
/// stays the operator's.
pub(crate) fn write_document(path: &Path, document: &Value) -> Result<(), InstallFailure> {
    let failure = |detail: String| InstallFailure::NotWritable {
        path: path.display().to_string(),
        detail,
    };
    let target = match hide_platform::fs::identity::canonical(path) {
        Ok(real) => real,
        Err(error) if error.kind() == ErrorKind::NotFound => path.to_path_buf(),
        Err(error) => return Err(failure(error.to_string())),
    };
    let parent = target
        .parent()
        .ok_or_else(|| failure("the path has no parent directory".to_owned()))?;
    fs::create_dir_all(parent).map_err(|error| failure(error.to_string()))?;
    let mut serialized =
        serde_json::to_string_pretty(document).map_err(|error| failure(error.to_string()))?;
    serialized.push('\n');
    hide_platform::fs::atomic::write_file(
        &target,
        serialized.as_bytes(),
        hide_platform::fs::Access::KeepOrPrivate,
    )
    .map(drop)
    .map_err(|error| failure(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn hide_can_take_out_its_own_entries_after_the_helper_they_name_is_gone() {
        let fixture = Fixture::new("helper-gone");
        let runtime = AgentRuntime::ClaudeCode;
        fs::create_dir_all(runtime.home_directory(fixture.home())).unwrap();
        let helper = fixture
            .home()
            .join("hide.app/Contents/Resources/hide-agent-hooks");
        fs::create_dir_all(helper.parent().unwrap()).unwrap();
        fs::write(&helper, b"binary").unwrap();
        install(runtime, fixture.home(), &helper).unwrap();

        // The build directory goes away, which is the whole incident.
        fs::remove_file(&helper).unwrap();
        let diagnosed = crate::Diagnosis::read(fixture.home());
        let row = diagnosed
            .runtimes
            .iter()
            .find(|row| row.runtime == runtime)
            .expect("the runtime is diagnosed");
        assert!(
            matches!(
                row.status,
                HookStatus::Failed {
                    reason: InstallFailure::HelperMissing { .. }
                }
            ),
            "got {:?}",
            row.status
        );
        assert!(
            row.offers_removal(),
            "the operator can take out entries whose helper is gone"
        );

        // And removal works without the binary, because it reads the file.
        let outcome = remove(runtime, fixture.home()).unwrap();
        assert!(outcome.changed);
        assert_eq!(outcome.removed_entries, HookEvent::ALL.len());
        assert!(matches!(
            status(runtime, fixture.home()),
            HookStatus::NotInstalled
        ));
    }

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "hide-agent-hooks-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|value| value.as_nanos())
                    .unwrap_or_default()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join(".claude")).unwrap();
            fs::create_dir_all(root.join(".codex")).unwrap();
            Self(root)
        }

        fn home(&self) -> &Path {
            &self.0
        }

        fn write(&self, runtime: AgentRuntime, body: &str) {
            fs::write(runtime.config_path(self.home()), body).unwrap();
        }

        fn read(&self, runtime: AgentRuntime) -> Value {
            let raw = fs::read_to_string(runtime.config_path(self.home())).unwrap();
            serde_json::from_str(&raw).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn helper(fixture: &Fixture) -> PathBuf {
        let path = fixture.home().join(crate::HELPER_BINARY_NAME);
        fs::write(&path, "#!/bin/sh\n").unwrap();
        path
    }

    /// The occupied array is the whole reason this crate exists: two other
    /// tools already sit on `SubagentStart` (PRD D-26).
    const OCCUPIED: &str = r#"{
      "model": "opus",
      "hooks": {
        "SubagentStart": [
          {"hooks": [{"type": "command", "command": "/opt/principles/inject.sh SubagentStart", "timeout": 5}]},
          {"hooks": [{"type": "command", "command": "/opt/orca/codex-hook.sh", "timeout": 10}]}
        ],
        "Stop": [
          {"hooks": [{"type": "command", "command": "/opt/checkpoint.sh"}]}
        ]
      }
    }"#;

    #[test]
    fn installing_appends_and_leaves_every_other_tools_entry_exactly_as_it_was() {
        let fixture = Fixture::new("append");
        fixture.write(AgentRuntime::Codex, OCCUPIED);
        let before = fixture.read(AgentRuntime::Codex);

        let outcome = install(AgentRuntime::Codex, fixture.home(), &helper(&fixture)).unwrap();
        assert!(outcome.changed);
        assert_eq!(outcome.preserved_entries, 3);

        let after = fixture.read(AgentRuntime::Codex);
        for event in ["SubagentStart", "Stop"] {
            let original = before["hooks"][event].as_array().unwrap();
            let current = after["hooks"][event].as_array().unwrap();
            assert_eq!(
                current.len(),
                original.len() + 1,
                "{event} gained one entry"
            );
            assert_eq!(
                &current[..original.len()],
                original.as_slice(),
                "{event} kept its own"
            );
        }
        assert_eq!(after["model"], before["model"], "unrelated keys survive");
        assert!(after["hooks"]["SessionStart"].as_array().unwrap().len() == 1);
        assert!(after["hooks"]["SubagentStop"].as_array().unwrap().len() == 1);
        let session_start_command = after["hooks"]["SessionStart"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(session_start_command.contains("--runtime codex"));
        assert!(session_start_command.contains("--memory-injection"));
        assert!(session_start_command.contains("--source hide-subagents@6"));
        let guard = if cfg!(windows) {
            "if (Test-Path -LiteralPath '"
        } else {
            "if [ -x '"
        };
        assert!(
            session_start_command.starts_with(guard),
            "the command runs the helper only while it is there"
        );
    }

    /// The bytes a Mac or Linux machine's files carry. Changing them makes
    /// every installed entry read as another build's, so they are pinned as
    /// `origin/main` at 91877ba9 wrote them, quote escaping included.
    #[test]
    fn the_posix_entry_is_exactly_what_macos_and_linux_have_installed() {
        let helper =
            Path::new("/Users/example/o'brien/hide.app/Contents/Resources/hide-agent-hooks");
        let pinned = [
            (
                AgentRuntime::ClaudeCode,
                HookEvent::SessionStart,
                r#"{"type":"command","command":"if [ -x '/Users/example/o'\\''brien/hide.app/Contents/Resources/hide-agent-hooks' ]; then exec '/Users/example/o'\\''brien/hide.app/Contents/Resources/hide-agent-hooks' hook --runtime claude-code --event SessionStart --memory-injection --source hide-subagents@6; fi","timeout":8}"#,
            ),
            (
                AgentRuntime::Codex,
                HookEvent::Stop,
                r#"{"type":"command","command":"if [ -x '/Users/example/o'\\''brien/hide.app/Contents/Resources/hide-agent-hooks' ]; then exec '/Users/example/o'\\''brien/hide.app/Contents/Resources/hide-agent-hooks' hook --runtime codex --event Stop --memory-injection --source hide-subagents@6; fi","timeout":8}"#,
            ),
        ];
        for (runtime, event, bytes) in pinned {
            let written = posix_hook(helper, runtime, event);
            assert_eq!(serde_json::to_string(&written).unwrap(), bytes);
            let command = written["command"].as_str().unwrap();
            assert_eq!(
                Quoting::Posix.first_quoted(command).as_deref(),
                helper.to_str()
            );
        }
        if !cfg!(windows) {
            assert_eq!(
                hook_group(helper, AgentRuntime::Codex, HookEvent::Stop),
                serde_json::json!({ "hooks": [posix_hook(helper, AgentRuntime::Codex, HookEvent::Stop)] })
            );
        }
    }

    /// Windows: Claude Code's entry runs PowerShell in exec form, Codex's
    /// command is the PowerShell guard its session shell runs. A path with
    /// a space, a quote and a typographic quote PowerShell also takes for
    /// one is read back as it was written, and the marker is found in
    /// either shape.
    #[test]
    fn the_windows_entries_are_powershell_guards_hide_reads_back() {
        let helper = Path::new("C:\\Users\\example\\a b'c\\it\u{2019}s\\hide-agent-hooks.exe");
        let quoted = "'C:\\Users\\example\\a b''c\\it\u{2019}\u{2019}s\\hide-agent-hooks.exe'";
        let guard = |runtime: &str| {
            format!(
                "if (Test-Path -LiteralPath {quoted} -PathType Leaf) {{ & {quoted} hook --runtime {runtime} --event Stop --memory-injection --source hide-subagents@6 }}"
            )
        };
        let codex = windows_hook(helper, AgentRuntime::Codex, HookEvent::Stop);
        assert_eq!(
            codex,
            serde_json::json!({ "type": "command", "command": guard("codex"), "timeout": 8 })
        );
        let claude = windows_hook(helper, AgentRuntime::ClaudeCode, HookEvent::Stop);
        assert_eq!(
            claude,
            serde_json::json!({
                "type": "command",
                "command": "powershell.exe",
                "args": ["-NoProfile", "-NonInteractive", "-Command", guard("claude-code")],
                "timeout": 8,
            })
        );
        for hook in [claude, codex] {
            let group = serde_json::json!({ "hooks": [hook] });
            assert_eq!(
                group_marker_version(&group),
                Some(crate::runtime::HOOK_VERSION)
            );
            let text = hook_texts(&group["hooks"][0])
                .find(|text| text.contains('\''))
                .unwrap();
            assert_eq!(
                Quoting::PowerShell.first_quoted(text).as_deref(),
                helper.to_str()
            );
        }
        if cfg!(windows) {
            assert_eq!(
                hook_group(helper, AgentRuntime::Codex, HookEvent::Stop),
                serde_json::json!({ "hooks": [windows_hook(helper, AgentRuntime::Codex, HookEvent::Stop)] })
            );
        }
    }

    #[test]
    fn installing_the_same_thing_twice_leaves_one_entry_per_event() {
        let fixture = Fixture::new("idempotent");
        fixture.write(AgentRuntime::Codex, OCCUPIED);
        let path = helper(&fixture);
        install(AgentRuntime::Codex, fixture.home(), &path).unwrap();
        let first = fixture.read(AgentRuntime::Codex);
        let second_outcome = install(AgentRuntime::Codex, fixture.home(), &path).unwrap();
        assert!(
            !second_outcome.changed,
            "a converged install writes nothing"
        );
        assert_eq!(fixture.read(AgentRuntime::Codex), first);
        for event in HookEvent::ALL {
            let groups = first["hooks"][event.name()].as_array().unwrap();
            assert_eq!(
                owned_versions(groups).len(),
                1,
                "{} has one Hide entry",
                event.name()
            );
        }
    }

    #[test]
    fn the_codex_command_the_trust_step_expects_is_what_install_writes() {
        // Codex hashes the command string it finds in the file, so the trust
        // step recognises Hide's entry by `codex_command` alone: on every
        // system it must be exactly what `install` wrote for each event, with
        // the matcher `hook_matcher` names.
        let fixture = Fixture::new("codex-command");
        let path = helper(&fixture);
        install(AgentRuntime::Codex, fixture.home(), &path).unwrap();
        let written = fixture.read(AgentRuntime::Codex);
        for event in HookEvent::ALL {
            let group = &written["hooks"][event.name()][0];
            assert_eq!(
                group["hooks"][0]["command"],
                codex_command(&path, event),
                "{}",
                event.name()
            );
            assert_eq!(
                group.get("matcher").and_then(Value::as_str),
                hook_matcher(AgentRuntime::Codex, event),
                "{}",
                event.name()
            );
        }
    }

    #[test]
    fn a_file_that_does_not_parse_is_never_written() {
        let fixture = Fixture::new("unparsable");
        let broken = "{\"hooks\": {\"Stop\": [ } ";
        fixture.write(AgentRuntime::Codex, broken);
        let error = install(AgentRuntime::Codex, fixture.home(), &helper(&fixture)).unwrap_err();
        assert!(matches!(error, InstallFailure::Unparsable { .. }));
        assert_eq!(
            fs::read_to_string(AgentRuntime::Codex.config_path(fixture.home())).unwrap(),
            broken,
            "the operator's bytes are untouched"
        );
        assert!(matches!(
            status(AgentRuntime::Codex, fixture.home()),
            HookStatus::Failed {
                reason: InstallFailure::Unparsable { .. }
            }
        ));
    }

    #[test]
    fn removal_takes_only_hides_entries_and_leaves_the_events_it_did_not_empty() {
        let fixture = Fixture::new("remove");
        fixture.write(AgentRuntime::Codex, OCCUPIED);
        install(AgentRuntime::Codex, fixture.home(), &helper(&fixture)).unwrap();

        let outcome = remove(AgentRuntime::Codex, fixture.home()).unwrap();
        assert!(outcome.changed);
        assert_eq!(outcome.removed_entries, HookEvent::ALL.len());
        assert_eq!(outcome.preserved_entries, 3);

        let after = fixture.read(AgentRuntime::Codex);
        let subagent_start = after["hooks"]["SubagentStart"].as_array().unwrap();
        assert_eq!(subagent_start.len(), 2);
        assert!(
            subagent_start[0]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .contains("inject.sh")
        );
        assert_eq!(after["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert!(
            after["hooks"].get("SessionStart").is_none(),
            "an event Hide emptied is dropped"
        );
        assert!(matches!(
            status(AgentRuntime::Codex, fixture.home()),
            HookStatus::NotInstalled
        ));
    }

    #[test]
    fn a_missing_runtime_directory_is_reported_rather_than_created() {
        let fixture = Fixture::new("absent");
        fs::remove_dir_all(fixture.home().join(".codex")).unwrap();
        assert_eq!(
            status(AgentRuntime::Codex, fixture.home()),
            HookStatus::RuntimeAbsent
        );
        assert!(!AgentRuntime::Codex.config_path(fixture.home()).exists());
    }

    #[test]
    fn an_older_marker_version_reads_as_outdated() {
        let fixture = Fixture::new("outdated");
        let helper_path = helper(&fixture);
        let quoted = Quoting::NATIVE.quote(&helper_path.display().to_string());
        let mut document = serde_json::json!({ "hooks": {} });
        for event in HookEvent::ALL {
            document["hooks"][event.name()] = serde_json::json!([{
                "hooks": [{
                    "type": "command",
                    "command": format!("{quoted} hook --event {} --source hide-subagents@0", event.name()),
                }]
            }]);
        }
        fixture.write(AgentRuntime::Codex, &document.to_string());
        assert_eq!(
            status(AgentRuntime::Codex, fixture.home()),
            HookStatus::Outdated { version: 0 }
        );
    }

    #[test]
    fn an_entry_whose_helper_is_gone_is_a_failure_not_a_healthy_install() {
        let fixture = Fixture::new("helper-gone");
        let helper_path = helper(&fixture);
        install(AgentRuntime::Codex, fixture.home(), &helper_path).unwrap();
        fs::remove_file(&helper_path).unwrap();
        assert!(matches!(
            status(AgentRuntime::Codex, fixture.home()),
            HookStatus::Failed {
                reason: InstallFailure::HelperMissing { .. }
            }
        ));
    }

    /// The command as `/bin/sh -c` runs it, which is how both runtimes run a
    /// command hook on Unix.
    #[cfg(unix)]
    fn run_hook_command(command: &str) -> std::process::ExitStatus {
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .stdin(std::process::Stdio::null())
            .status()
            .unwrap()
    }

    /// The guard is shell syntax and the helper here is a shell script, so
    /// this runs where `/bin/sh` does.
    #[cfg(unix)]
    #[test]
    fn a_hook_whose_helper_is_gone_succeeds_and_one_that_is_there_runs() {
        let fixture = Fixture::new("guard");
        // A quote in the folder name is the case the path reader must undo.
        let helper = fixture
            .home()
            .join("it's here")
            .join(crate::HELPER_BINARY_NAME);
        install(AgentRuntime::Codex, fixture.home(), &helper).unwrap();
        assert_eq!(
            installed_helper_path(AgentRuntime::Codex, fixture.home()).as_deref(),
            Some(helper.to_str().unwrap()),
            "the path is read back from the guarded command"
        );
        let command = fixture.read(AgentRuntime::Codex)["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .to_owned();

        // The app was moved to the Trash: the agent's turn goes on (B3).
        assert!(run_hook_command(&command).success());

        let ran = fixture.home().join("ran");
        fs::create_dir_all(helper.parent().unwrap()).unwrap();
        fs::write(
            &helper,
            format!("#!/bin/sh\nprintf '%s' \"$*\" > '{}'\n", ran.display()),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(run_hook_command(&command).success());
        let arguments = fs::read_to_string(&ran).unwrap();
        assert!(
            arguments.starts_with("hook --runtime codex --event Stop"),
            "{arguments}"
        );
    }

    /// A settings file can hold tokens, and a dotfile manager can own it as
    /// a link: installing and removing keep its permissions and its link.
    #[test]
    fn installing_and_removing_keep_the_files_permissions_and_its_link() {
        use hide_platform::fs::link;
        use hide_platform::fs::permissions::Permissions;
        let kept = |path: &Path| Permissions::of(&fs::File::open(path).unwrap()).unwrap();
        let fixture = Fixture::new("mode-link");
        let claude = AgentRuntime::ClaudeCode.config_path(fixture.home());
        fixture.write(AgentRuntime::ClaudeCode, OCCUPIED);
        let dotfiles = fixture.home().join("dotfiles");
        fs::create_dir_all(&dotfiles).unwrap();
        let real = dotfiles.join("hooks.json");
        fs::write(&real, OCCUPIED).unwrap();
        // The permissions that matter are the mode bits, and only Unix has
        // them; elsewhere the files keep what the system gave them.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&claude, fs::Permissions::from_mode(0o600)).unwrap();
            fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
        }
        let codex = AgentRuntime::Codex.config_path(fixture.home());
        match link::create_link(&real, &codex) {
            Ok(()) => {}
            // A Windows account with neither the privilege nor Developer
            // Mode cannot make the link this test needs.
            Err(error) if link::needs_privilege(&error) => return,
            Err(error) => panic!("{error}"),
        }
        let (claude_before, real_before) = (kept(&claude), kept(&real));
        #[cfg(unix)]
        {
            assert_eq!(claude_before.unix_mode(), Some(0o600));
            assert_eq!(real_before.unix_mode(), Some(0o640));
        }

        for runtime in [AgentRuntime::ClaudeCode, AgentRuntime::Codex] {
            assert!(
                install(runtime, fixture.home(), &helper(&fixture))
                    .unwrap()
                    .changed
            );
        }
        assert_eq!(kept(&claude), claude_before);
        assert!(link::is_link_to(&codex, &real));
        assert_eq!(kept(&real), real_before);
        assert!(
            fs::read_to_string(&real)
                .unwrap()
                .contains(crate::HELPER_BINARY_NAME)
        );

        for runtime in [AgentRuntime::ClaudeCode, AgentRuntime::Codex] {
            remove(runtime, fixture.home()).unwrap();
        }
        assert_eq!(kept(&claude), claude_before);
        assert!(link::is_link_to(&codex, &real));
        assert_eq!(
            fixture.read(AgentRuntime::Codex),
            serde_json::from_str::<Value>(OCCUPIED).unwrap()
        );
    }

    #[test]
    fn a_runtime_with_no_config_file_yet_installs_into_a_new_one() {
        let fixture = Fixture::new("fresh");
        assert_eq!(
            status(AgentRuntime::ClaudeCode, fixture.home()),
            HookStatus::NotInstalled
        );
        install(AgentRuntime::ClaudeCode, fixture.home(), &helper(&fixture)).unwrap();
        assert_eq!(
            status(AgentRuntime::ClaudeCode, fixture.home()),
            HookStatus::Installed {
                version: crate::runtime::HOOK_VERSION
            }
        );
    }
}
