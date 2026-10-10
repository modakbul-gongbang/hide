//! `hided core-move <step>`: the steps of a move that run on the other
//! machine, each one SSH exec from the driver (PRD core-host-node-move B4,
//! B5). Each prints one JSON line and exits 0 once it has an answer, a
//! refusal included; a usage error exits non-zero.
//!
//! On the machine taking the core: `inspect`, `verify`, `place`, `start`,
//! `status`, `abort` and `finish`. On the machine giving the core back to
//! the node that dialed it (`back`): `release`, `resume` and `retire`.
//! Every step that changes the state folder runs under the handover
//! record's lock (`handover`), and each can be run again with the same
//! intent and reach the same state.

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
    /// For `inspect`: the agents Hide AI asks, as `[[provider, model]]`.
    ai: Option<String>,
}

const USAGE: &str = "usage: hided core-move <inspect|verify|place|start|status|abort|finish|release|resume|retire> [--state-dir <dir>] [--intent <id>] [--source <node>] [--target <node>] [--ai <json>]";

/// How long a released core may take to end once it answered.
const RELEASED_WITHIN: std::time::Duration = std::time::Duration::from_secs(30);

fn parse(args: &[OsString]) -> Result<Args, String> {
    let mut args = args.iter();
    let step = args
        .next()
        .and_then(|step| step.to_str())
        .ok_or(USAGE)?
        .to_owned();
    let (mut state_dir, mut intent, mut source, mut target, mut ai) =
        (None, None, None, None, None);
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
            Some("--ai") => ai = Some(value),
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
        ai,
    })
}

pub fn run(args: &[OsString]) -> Result<(), String> {
    let args = parse(args)?;
    let answer = match args.step.as_str() {
        "inspect" => inspect(&args.state_dir, args.ai.as_deref()),
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
                "release" => release(
                    &args.state_dir,
                    intent,
                    args.target.as_deref().ok_or(USAGE)?,
                ),
                "resume" => resume(&args.state_dir, intent),
                "retire" => retire(&args.state_dir, intent),
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

pub(crate) fn incoming(state_dir: &Path, intent: &str) -> PathBuf {
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
/// the move touching it, its build and Hide AI settings, and each of the
/// move's checks that fails here (`preflight`), Hide AI's agents in `ai`
/// among them.
fn inspect(state_dir: &Path, ai: Option<&str>) -> Result<Value, Value> {
    let node = herdr_core::node::NodeId::of_this_machine().map_err(plain)?;
    let handover = handover::read(state_dir).map_err(plain)?;
    // The Herdr this machine's core would own, as its daemon resolves it.
    let env = crate::env::load()
        .map_err(|errors| plain(format!("{} environment errors", errors.len())))?;
    let mut failed = super::preflight::failing(
        &env.home,
        &super::preflight::Herdr {
            bin: env.herdr_bin_path.as_deref(),
            socket: env.herdr_socket_path.as_deref(),
        },
        &super::preflight::Programs::for_this_machine(),
    );
    if let Some(asks) = ai {
        let asks: Vec<(String, String)> = serde_json::from_str(asks)
            .map_err(|error| plain(format!("--ai is not a list of agents: {error}")))?;
        if let Err(detail) = herdr_core::hide_ai_ready_here(&asks) {
            failed.push(super::control::FailedCheck {
                check: super::control::CheckId::Ai,
                detail,
            });
        }
    }
    let ai = match herdr_core::stored_hide_ai_settings(&env.home) {
        Ok(settings) => json!({"settings": settings}),
        Err(error) => json!({"unreadable": error}),
    };
    Ok(json!({
        "herdr_socket": env.herdr_socket_path,
        "node": node.as_str(),
        "state_dir": hide_platform::path::to_wire(state_dir).map_err(|error| plain(error.to_string()))?,
        "brain": copy::brain_present(state_dir),
        "handover": handover,
        "build": crate::build_id::of_current_exe().map_err(plain)?,
        "ai": ai,
        "failed": failed,
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
        // A core this machine retired for a move back holds nothing.
        Some(record) if record.state == HandoverState::Retired => {}
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
        Err(not_placed) => {
            // What the folder holds now is its own, unless part of the copy
            // could not be taken back: that stays pending, for the driver's
            // abort to take back.
            if not_placed.left.is_empty() {
                held.remove().map_err(plain)?;
            }
            Err(refusal(not_placed.refusal))
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
    // A core still starting holds no record yet, but it holds the instance
    // lock; holding it here keeps one from starting on the copy while it is
    // taken back.
    let _instance = acquire_instance(state_dir)?;
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

/// Has this machine's core stop for the move back `intent` to `target` and
/// stages its copy: the core checks the move, records the stop and its
/// export, answers, and ends; the copy is made once it has ended, under the
/// instance lock so no core starts on the folder meanwhile.
fn release(state_dir: &Path, intent: &str, target: &str) -> Result<Value, Value> {
    match handover::read(state_dir).map_err(plain)? {
        Some(record) if record.intent == intent && record.state == HandoverState::Retired => {
            return Ok(json!({"state": "retired"}));
        }
        // Stopped already, by an earlier run of this step.
        Some(record) if record.intent == intent && record.state == HandoverState::StoppedFor => {}
        Some(record) if record.state != HandoverState::Retired => {
            return Err(plain(format!(
                "another move ({}) holds this machine",
                record.intent
            )));
        }
        _ => {
            let running = crate::state_file::read_state(state_dir)
                .map_err(|error| plain(error.to_string()))?
                .ok_or_else(|| plain("no core runs here"))?;
            if let Err(reason) = crate::attach::release(state_dir, intent, target) {
                // The answer can be lost as the core ends; its record says
                // whether it stopped.
                let stopped = handover::read(state_dir)
                    .ok()
                    .flatten()
                    .is_some_and(|record| {
                        record.intent == intent && record.state == HandoverState::StoppedFor
                    });
                if !stopped {
                    return Err(plain(reason));
                }
            }
            wait_until_gone(running.pid)?;
        }
    }
    let _instance = acquire_instance(state_dir)?;
    let export = super::back::read_export(state_dir, intent).map_err(plain)?;
    let staging = herdr_core::node_migration::staging_dir(state_dir, intent);
    copy::stage(state_dir, &staging).map_err(refusal)?;
    super::driver::carry_labels(&staging, target, &export.labels).map_err(plain)?;
    let manifest = copy::digest(&staging).map_err(refusal)?;
    let bytes = serde_json::to_vec(&manifest).map_err(|error| plain(error.to_string()))?;
    let path = super::back::manifest_path(state_dir, intent);
    hide_platform::fs::atomic::write_file_durable(
        &path,
        &bytes,
        hide_platform::fs::Access::Private,
    )
    .map_err(|error| plain(format!("{}: {error}", path.display())))?;
    Ok(json!({"state": "staged", "files": manifest.files.len()}))
}

/// Undoes a release: the staged copy goes and this machine's core starts on
/// its folder again. A core retired for the move stays retired.
fn resume(state_dir: &Path, intent: &str) -> Result<Value, Value> {
    let held = handover::hold(state_dir).map_err(plain)?;
    match held.read().map_err(plain)? {
        Some(record) if record.intent == intent && record.state == HandoverState::Retired => {
            return Ok(json!({"state": "retired"}));
        }
        Some(record) if record.intent == intent && record.state == HandoverState::StoppedFor => {
            super::back::remove_staging(state_dir, intent).map_err(plain)?;
            held.remove().map_err(plain)?;
        }
        Some(record) if record.state != HandoverState::Retired => {
            return Err(plain(format!(
                "another move ({}) holds this machine",
                record.intent
            )));
        }
        // The release never stopped the core, or an earlier resume ran.
        _ => {}
    }
    drop(held);
    if let Ok(Some(running)) = crate::state_file::read_state(state_dir)
        && hide_platform::process::is_alive(running.pid)
    {
        return Ok(json!({"state": "running", "pid": running.pid}));
    }
    let home = hide_platform::host::home_dir().map_err(|error| plain(error.to_string()))?;
    let pid = CoreStarter::for_account(&home)
        .and_then(|starter| starter.start(state_dir))
        .map_err(plain)?;
    Ok(json!({"state": "running", "pid": pid}))
}

/// The commit of a move back: this machine's starter is removed, so no
/// core of its starts again, and its brain state is set aside.
fn retire(state_dir: &Path, intent: &str) -> Result<Value, Value> {
    let held = handover::hold(state_dir).map_err(plain)?;
    match held.read().map_err(plain)? {
        Some(record) if record.intent == intent && record.state == HandoverState::Retired => {}
        Some(mut record)
            if record.intent == intent && record.state == HandoverState::StoppedFor =>
        {
            let home = hide_platform::host::home_dir().map_err(|error| plain(error.to_string()))?;
            CoreStarter::for_account(&home)
                .and_then(|starter| starter.stop(state_dir))
                .map_err(plain)?;
            let _instance = acquire_instance(state_dir)?;
            record.state = HandoverState::Retired;
            held.write(&record).map_err(plain)?;
        }
        _ => return Err(plain("this machine's core is not stopped for this move")),
    }
    copy::set_aside(state_dir, intent).map_err(refusal)?;
    super::back::remove_staging(state_dir, intent).map_err(plain)?;
    Ok(json!({"state": "retired"}))
}

/// Waits until `pid` has ended.
#[allow(clippy::disallowed_methods)] // a production wait, not test code
fn wait_until_gone(pid: u32) -> Result<(), Value> {
    let deadline = std::time::Instant::now() + RELEASED_WITHIN;
    while hide_platform::process::is_alive(pid) {
        if std::time::Instant::now() >= deadline {
            return Err(plain(format!(
                "the core {pid} did not end after its release"
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}

/// The folder's instance lock, which no core holds once this has it.
fn acquire_instance(state_dir: &Path) -> Result<std::fs::File, Value> {
    crate::state_file::acquire_lock(state_dir).map_err(|error| {
        plain(format!(
            "a core of this folder is still starting or running: {error}"
        ))
    })
}
