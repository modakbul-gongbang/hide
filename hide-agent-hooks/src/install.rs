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
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

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
/// Claims the one automatic install this machine gets, and reports whether
/// this call is the one that got it.
///
/// Hide installs its hooks once, on first run, and then leaves the operator's
/// configuration alone; an operator who removes a hook has removed it, and
/// the next launch must not quietly put it back (PRD B25, D-31). The claim is
/// a marker file created exclusively, so two launches racing each other still
/// install once, and it lives in Hide's own directory rather than in the
/// rendered UI state, which the shell echoes back and could reset.
pub fn claim_first_run(home: &Path) -> io::Result<bool> {
    let path = crate::counters::state_directory(home)
        .parent()
        .expect("the counter directory is always nested")
        .join("installed-once");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

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
        groups.push(hook_group(helper, event));
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

/// The entry Hide writes: one command hook, carrying its own marker.
fn hook_group(helper: &Path, event: HookEvent) -> Value {
    let command = format!(
        "{} hook --event {} --source {}",
        shell_quote(&helper.display().to_string()),
        event.name(),
        hook_source_id()
    );
    serde_json::json!({
        "hooks": [{
            "type": "command",
            "command": command,
            "timeout": 5,
        }]
    })
}

/// Single-quote a path for `/bin/sh`, the shell both runtimes run a command
/// hook through.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn group_marker_version(group: &Value) -> Option<u32> {
    group
        .get("hooks")?
        .as_array()?
        .iter()
        .filter_map(|hook| hook.get("command")?.as_str())
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
        .filter_map(|group| {
            group
                .get("hooks")?
                .as_array()?
                .first()?
                .get("command")?
                .as_str()
        })
        .find_map(parse_quoted_helper)
}

/// Reads the leading `'…'` back out of a command Hide wrote.
fn parse_quoted_helper(command: &str) -> Option<String> {
    let rest = command.strip_prefix('\'')?;
    let end = rest.find('\'')?;
    Some(rest[..end].to_owned())
}

fn read_document(path: &Path) -> Result<Option<Value>, InstallFailure> {
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

/// Writes through a temporary file in the same directory, so a failure part
/// way through leaves the operator's original file intact.
fn write_document(path: &Path, document: &Value) -> Result<(), InstallFailure> {
    let failure = |detail: String| InstallFailure::NotWritable {
        path: path.display().to_string(),
        detail,
    };
    let parent = path
        .parent()
        .ok_or_else(|| failure("the path has no parent directory".to_owned()))?;
    fs::create_dir_all(parent).map_err(|error| failure(error.to_string()))?;
    let mut serialized =
        serde_json::to_string_pretty(document).map_err(|error| failure(error.to_string()))?;
    serialized.push('\n');
    let temporary: PathBuf = path.with_extension("hide-tmp");
    {
        let mut file = fs::File::create(&temporary).map_err(|error| failure(error.to_string()))?;
        file.write_all(serialized.as_bytes())
            .map_err(|error| failure(error.to_string()))?;
        file.sync_all()
            .map_err(|error| failure(error.to_string()))?;
    }
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        failure(error.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_automatic_install_is_claimed_once_and_a_removed_hook_stays_removed() {
        let fixture = Fixture::new("first-run");
        assert!(
            claim_first_run(fixture.home()).unwrap(),
            "the first launch on this machine installs"
        );
        assert!(
            !claim_first_run(fixture.home()).unwrap(),
            "every later launch leaves the operator's configuration alone"
        );
        // The claim does not depend on any hook file, so removing one does
        // not hand the next launch a fresh install (PRD D-31).
        for runtime in AgentRuntime::ALL {
            let _ = fs::remove_file(runtime.config_path(fixture.home()));
        }
        assert!(!claim_first_run(fixture.home()).unwrap());
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
        let quoted = shell_quote(&helper_path.display().to_string());
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
