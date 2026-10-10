//! `hided core-update`: the core on this machine is updated to this build
//! (PRD core-host-node-move B10, D-10). A node of a newer build runs it here
//! over SSH, once it put this build in a version folder beside the one the
//! core runs (`hide_node::ssh::upstream::Upstream::install_build`), so the
//! update needs nothing from the older core. It prints one JSON line and
//! exits 0 once it has an answer, a refusal included; a usage error exits
//! non-zero.
//!
//! The core is replaced by this build's through its starter (the account's
//! login item, whose replacement ends the old job), and has 30 s to take
//! links; one that does not is replaced by the previous build's again. Both
//! ends leave `current` at the build that runs and keep the other build of
//! the update, so a later connection reuses its files and no more than two
//! builds are kept. A move holds this machine's handover record while it
//! runs, and the update holds that record's lock for its whole length, so
//! the two never overlap: an update finds a move's record and is refused as
//! `move_running`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::core_move::handover;
use crate::core_move::starter::CoreStarter;

const USAGE: &str = "usage: hided core-update --state-dir <dir> --previous <hided> --intent <id>";

/// How long an update may take where it runs: the old core's stop, the new
/// one's 30 s to take links, and the same again for a way back.
pub const WITHIN: std::time::Duration = std::time::Duration::from_secs(150);
/// The most an update's run may print that its caller reads.
pub const OUTPUT_CAP: usize = 64 * 1024;

/// A new update's intent id.
pub fn new_intent() -> String {
    crate::core_move::new_intent().replacen("move-", "update-", 1)
}

/// What an update's run prints, its one line on standard output. The
/// node that runs it uploaded this build, so both ends read one shape.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Outcome {
    /// The core runs this build.
    Updated {
        pid: u32,
        program: PathBuf,
    },
    /// This build did not start, and the previous build's core runs again.
    RolledBack {
        pid: u32,
        reason: String,
    },
    Refused {
        reason: String,
    },
}

/// What an update's run said: `Ok` once the core runs the new build, the
/// reason otherwise. A line that is not an outcome is named by the run's
/// exit, never read as one.
pub fn outcome(stdout: &str, exit: &str, stderr: &str) -> Result<(), String> {
    let Some(outcome) = stdout
        .lines()
        .last()
        .and_then(|line| serde_json::from_str::<Outcome>(line).ok())
    else {
        return Err(format!(
            "core-update exited {exit}: {}",
            stderr.trim().chars().take(512).collect::<String>()
        ));
    };
    match outcome {
        Outcome::Updated { .. } => Ok(()),
        Outcome::RolledBack { reason, .. } => Err(format!("rolled_back: {reason}")),
        Outcome::Refused { reason } => Err(reason),
    }
}

struct Args {
    state_dir: PathBuf,
    previous: PathBuf,
    intent: String,
}

fn parse(args: &[OsString]) -> Result<Args, String> {
    let (mut state_dir, mut previous, mut intent) = (None, None, None);
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .and_then(|value| value.to_str())
            .ok_or(USAGE)?
            .to_owned();
        match flag.to_str() {
            Some("--state-dir") if Path::new(&value).is_absolute() => {
                state_dir = Some(PathBuf::from(value));
            }
            Some("--previous") if Path::new(&value).is_absolute() => {
                previous = Some(PathBuf::from(value));
            }
            Some("--intent") => {
                intent = Some(crate::core_move::checked_intent(&value)?.to_owned());
            }
            _ => return Err(USAGE.to_owned()),
        }
    }
    Ok(Args {
        state_dir: state_dir.ok_or(USAGE)?,
        previous: previous.ok_or(USAGE)?,
        intent: intent.ok_or(USAGE)?,
    })
}

pub fn run(args: &[OsString]) -> Result<(), String> {
    let args = parse(args)?;
    let outcome = update(&args).unwrap_or_else(|reason| Outcome::Refused { reason });
    herdr_core::diagnostic!(json!({
        "component": "core_update",
        "kind": "update.answered",
        "intent": args.intent,
        "answer": outcome,
    }));
    println!(
        "{}",
        serde_json::to_string(&outcome).map_err(|error| error.to_string())?
    );
    Ok(())
}

/// A build's place: `<root>/<version>/hided`, the layout the kit installs
/// and a core's login item runs.
#[derive(Debug, Eq, PartialEq)]
struct Build {
    root: PathBuf,
    version: String,
}

fn build_of(program: &Path) -> Result<Build, String> {
    let named = |path: Option<&Path>| {
        path.and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .map(str::to_owned)
    };
    let version = named(program.parent())
        .filter(|version| hide_kit::legacy::is_build_name(version))
        .ok_or_else(|| format!("{} is not in a build folder", program.display()))?;
    if named(Some(program)).as_deref() != Some("hided") {
        return Err(format!("{} is not a build's hided", program.display()));
    }
    let root = program
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| format!("{} has no build root", program.display()))?;
    Ok(Build {
        root: root.to_path_buf(),
        version,
    })
}

fn update(args: &Args) -> Result<Outcome, String> {
    let program =
        std::env::current_exe().map_err(|error| format!("this hided has no path: {error}"))?;
    let new = build_of(&program)?;
    let previous = build_of(&args.previous)?;
    if new.root != previous.root {
        return Err(format!(
            "{} and {} are not builds of one install",
            program.display(),
            args.previous.display()
        ));
    }
    let held = handover::hold(&args.state_dir)?;
    if let Some(record) = held.read()? {
        log(
            &args.intent,
            "update.refused",
            json!({"move": record.intent}),
        );
        return Err("move_running".to_owned());
    }
    let home = hide_platform::host::home_dir().map_err(|error| error.to_string())?;
    let starter = CoreStarter::for_account(&home)?;
    log(
        &args.intent,
        "update.started",
        json!({"previous": previous.version, "build": new.version}),
    );
    match starter.replace(&args.state_dir, &program) {
        Ok(pid) => {
            adopt(&new, &previous.version);
            log(&args.intent, "update.done", json!({"pid": pid}));
            Ok(Outcome::Updated { pid, program })
        }
        Err(reason) => {
            log(&args.intent, "update.failed", json!({"reason": reason}));
            let pid = starter
                .replace(&args.state_dir, &args.previous)
                .map_err(|again| {
                    log(&args.intent, "rollback.failed", json!({"reason": again}));
                    format!("the update failed ({reason}) and the previous build did not start again: {again}")
                })?;
            adopt(&previous, &new.version);
            log(&args.intent, "rollback.done", json!({"pid": pid}));
            Ok(Outcome::RolledBack { pid, reason })
        }
    }
}

/// `current` leads to `running`, and the other build of the update stays.
fn adopt(running: &Build, other: &str) {
    if let Err(error) = hide_host::kit::adopt_build(&running.root, &running.version, other) {
        // The core runs either way; the kit's next pass points it again.
        herdr_core::diagnostic!(json!({
            "component": "core_update",
            "kind": "current.failed",
            "reason": error.to_string(),
        }));
    }
}

fn log(intent: &str, kind: &str, fields: Value) {
    let mut record = json!({"component": "core_update", "kind": kind, "intent": intent});
    if let (Some(record), Value::Object(fields)) = (record.as_object_mut(), fields) {
        record.extend(fields);
    }
    herdr_core::diagnostic!(record);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The node reads only an outcome: a line of any other shape is named
    /// by the run's exit, not read as an update.
    #[test]
    fn an_update_is_read_only_from_its_outcome() {
        let line = |outcome: &Outcome| serde_json::to_string(outcome).unwrap();
        let updated = Outcome::Updated {
            pid: 7,
            program: PathBuf::from("/h/b/hided"),
        };
        assert_eq!(outcome(&line(&updated), "0", ""), Ok(()));
        assert_eq!(
            outcome(
                &line(&Outcome::RolledBack {
                    pid: 8,
                    reason: "no links".to_owned()
                }),
                "0",
                ""
            ),
            Err("rolled_back: no links".to_owned())
        );
        assert_eq!(
            outcome(r#"{"updated":{"pid":7}}"#, "0", "old"),
            Err("core-update exited 0: old".to_owned())
        );
    }

    #[test]
    fn only_a_build_folder_s_hided_is_a_build() {
        assert_eq!(
            build_of(Path::new("/h/.hide/host-helper/0123456789abcdef/hided")),
            Ok(Build {
                root: PathBuf::from("/h/.hide/host-helper"),
                version: "0123456789abcdef".to_owned(),
            })
        );
        for program in [
            "/h/.hide/host-helper/current/hided",
            "/h/.hide/host-helper/0123456789abcdef/hide",
            "/usr/local/bin/hided",
        ] {
            assert!(build_of(Path::new(program)).is_err(), "{program}");
        }
    }
}
