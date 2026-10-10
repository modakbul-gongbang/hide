//! `hided core-move <step>`: the steps of a move that run on the other
//! machine, each one SSH exec from the driver (PRD core-host-node-move B4,
//! B5). Each prints one JSON line and exits 0 once it has an answer, a
//! refusal included; a usage error exits non-zero.
//!
//! On the machine taking the core: `inspect`, `verify`, `place`, `start`,
//! `status`, `abort` and `finish`. Every step that changes the state folder
//! runs under the handover record's lock (`handover`), and each can be run
//! again with the same intent and reach the same state.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use herdr_core::node_migration::copy;
use serde_json::{Value, json};

use super::handover::{self, Handover, HandoverState};
use super::starter::CoreStarter;

struct Args {
    step: String,
    state_dir: PathBuf,
    intent: Option<String>,
    source: Option<String>,
    target: Option<String>,
}

const USAGE: &str = "usage: hided core-move <inspect|verify|place|start|status|abort|finish> [--state-dir <dir>] [--intent <id>] [--source <node>] [--target <node>]";

fn parse(args: &[OsString]) -> Result<Args, String> {
    let mut args = args.iter();
    let step = args
        .next()
        .and_then(|step| step.to_str())
        .ok_or(USAGE)?
        .to_owned();
    let (mut state_dir, mut intent, mut source, mut target) = (None, None, None, None);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .and_then(|value| value.to_str())
            .ok_or(USAGE)?
            .to_owned();
        match flag.to_str() {
            Some("--state-dir") => {
                if !Path::new(&value).is_absolute() {
                    return Err("--state-dir must be an absolute path".to_owned());
                }
                state_dir = Some(PathBuf::from(value));
            }
            Some("--intent") => intent = Some(super::checked_intent(&value)?.to_owned()),
            Some("--source") => source = Some(value),
            Some("--target") => target = Some(value),
            _ => return Err(USAGE.to_owned()),
        }
    }
    let state_dir = match state_dir {
        Some(dir) => dir,
        None => {
            let home = hide_platform::host::home_dir()
                .map_err(|error| format!("core-move has no home folder: {error}"))?;
            hide_kit::layout::state_dir_from_process(&home)
        }
    };
    Ok(Args {
        step,
        state_dir,
        intent,
        source,
        target,
    })
}

pub fn run(args: &[OsString]) -> Result<(), String> {
    let args = parse(args)?;
    let answer = match args.step.as_str() {
        "inspect" => inspect(&args.state_dir),
        step => {
            let intent = args.intent.as_deref().ok_or(USAGE)?;
            match step {
                "verify" => verify(&args.state_dir, intent),
                "place" => place(
                    &args.state_dir,
                    intent,
                    args.source.as_deref().ok_or(USAGE)?,
                    args.target.as_deref().ok_or(USAGE)?,
                ),
                "start" => start(&args.state_dir, intent),
                "status" => status(&args.state_dir, intent),
                "abort" => abort(&args.state_dir, intent),
                "finish" => finish(&args.state_dir, intent),
                _ => return Err(USAGE.to_owned()),
            }
        }
    };
    let line = match answer {
        Ok(value) => value,
        Err(refusal) => json!({"refused": refusal}),
    };
    herdr_core::diagnostic!(json!({
        "component": "core_move",
        "kind": "target.step",
        "step": args.step,
        "intent": args.intent,
        "answer": line,
    }));
    println!("{line}");
    Ok(())
}

pub(super) fn incoming(state_dir: &Path, intent: &str) -> PathBuf {
    hide_kit::layout::move_incoming(state_dir).join(intent)
}

pub(super) fn manifest_path(state_dir: &Path, intent: &str) -> PathBuf {
    hide_kit::layout::move_incoming(state_dir).join(format!("{intent}.manifest.json"))
}

fn refusal(error: herdr_core::node_migration::Refusal) -> Value {
    json!({"file": error.file.display().to_string(), "reason": error.reason})
}

fn plain(reason: impl Into<String>) -> Value {
    json!({"reason": reason.into()})
}

/// This machine's node, state folder, what brain state the folder holds,
/// and the move touching it.
fn inspect(state_dir: &Path) -> Result<Value, Value> {
    let node = herdr_core::node::NodeId::of_this_machine().map_err(plain)?;
    let handover = handover::read(state_dir).map_err(plain)?;
    // The socket this machine's core would own, as its daemon resolves it.
    let herdr_socket = crate::env::load()
        .map_err(|errors| plain(format!("{} environment errors", errors.len())))?
        .herdr_socket_path;
    Ok(json!({
        "herdr_socket": herdr_socket,
        "node": node.as_str(),
        "state_dir": hide_platform::path::to_wire(state_dir).map_err(|error| plain(error.to_string()))?,
        "brain": copy::brain_present(state_dir),
        "handover": handover,
    }))
}

/// Compares the received copy with the manifest sent beside it; a copy
/// that matches is loaded with this build's readers.
fn verify(state_dir: &Path, intent: &str) -> Result<Value, Value> {
    let path = manifest_path(state_dir, intent);
    let bytes =
        std::fs::read(&path).map_err(|error| plain(format!("{}: {error}", path.display())))?;
    let manifest: copy::Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| plain(format!("{}: {error}", path.display())))?;
    let dir = incoming(state_dir, intent);
    let received = if dir.exists() {
        copy::digest(&dir).map_err(refusal)?
    } else {
        copy::Manifest::default()
    };
    let differs = manifest.differs(&received);
    let extra: Vec<&String> = received
        .files
        .keys()
        .filter(|path| !manifest.files.contains_key(*path))
        .collect();
    if !differs.is_empty() || !extra.is_empty() {
        return Ok(json!({"differs": differs, "extra": extra}));
    }
    copy::check_loadable(&dir).map_err(refusal)?;
    Ok(json!({"loadable": true}))
}

/// Places the received copy, under a pending handover for `intent`.
fn place(state_dir: &Path, intent: &str, source: &str, target: &str) -> Result<Value, Value> {
    let held = handover::hold(state_dir).map_err(plain)?;
    match held.read().map_err(plain)? {
        Some(record) if record.intent != intent => {
            return Err(plain(format!(
                "another move ({}) holds this machine",
                record.intent
            )));
        }
        Some(record) => {
            // Placed already, by an earlier run of this step.
            return Ok(json!({"handover": record}));
        }
        None => {}
    }
    let record = Handover::new(intent, source, target, HandoverState::Pending);
    held.write(&record).map_err(plain)?;
    let placed = copy::place(&incoming(state_dir, intent), state_dir);
    match placed {
        Ok(placed) => Ok(json!({"placed": placed, "handover": record})),
        Err(error) => {
            // Nothing of a half-placed copy stays as brain state.
            let undone = copy::unplace(state_dir, &incoming(state_dir, intent));
            if undone.is_ok() {
                held.remove().map_err(plain)?;
            }
            Err(refusal(error))
        }
    }
}

fn start(state_dir: &Path, intent: &str) -> Result<Value, Value> {
    match handover::read(state_dir).map_err(plain)? {
        Some(record) if record.intent == intent => {}
        _ => return Err(plain("no copy of this move is placed here")),
    }
    let home = hide_platform::host::home_dir().map_err(|error| plain(error.to_string()))?;
    let starter = CoreStarter::for_account(&home).map_err(plain)?;
    let pid = starter.start(state_dir).map_err(plain)?;
    Ok(json!({"pid": pid}))
}

fn status(state_dir: &Path, intent: &str) -> Result<Value, Value> {
    let record = handover::read(state_dir).map_err(plain)?;
    Ok(match record {
        Some(record) if record.intent == intent => json!({"handover": record}),
        Some(record) => json!({"other": record.intent}),
        None => json!({"handover": null}),
    })
}

/// Stops a pending core and returns its copy to `move-incoming`; an active
/// core is the move committed and is left running.
fn abort(state_dir: &Path, intent: &str) -> Result<Value, Value> {
    let held = handover::hold(state_dir).map_err(plain)?;
    match held.read().map_err(plain)? {
        None => return Ok(json!({"state": "aborted"})),
        Some(record) if record.intent != intent => {
            return Err(plain(format!(
                "another move ({}) holds this machine",
                record.intent
            )));
        }
        Some(record) if record.state == HandoverState::Active => {
            return Ok(json!({"state": "active"}));
        }
        Some(_) => {}
    }
    let home = hide_platform::host::home_dir().map_err(|error| plain(error.to_string()))?;
    CoreStarter::for_account(&home)
        .and_then(|starter| starter.stop(state_dir))
        .map_err(plain)?;
    copy::unplace(state_dir, &incoming(state_dir, intent)).map_err(refusal)?;
    held.remove().map_err(plain)?;
    Ok(json!({"state": "aborted"}))
}

/// Ends the move's records once it committed: the handover and the copy it
/// was placed from.
fn finish(state_dir: &Path, intent: &str) -> Result<Value, Value> {
    let held = handover::hold(state_dir).map_err(plain)?;
    match held.read().map_err(plain)? {
        Some(record) if record.intent == intent && record.state == HandoverState::Active => {
            held.remove().map_err(plain)?;
        }
        Some(record) if record.intent == intent => {
            return Err(plain("the move has not committed here"));
        }
        _ => {}
    }
    for path in [
        incoming(state_dir, intent),
        manifest_path(state_dir, intent),
    ] {
        let removed = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        match removed {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(plain(format!("{}: {error}", path.display()))),
        }
    }
    Ok(json!({"state": "done"}))
}
