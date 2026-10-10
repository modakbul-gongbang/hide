//! The driver's steps of a forward move that wait on files or on the other
//! machine (PRD core-host-node-move B4, B5): each runs on a blocking
//! thread, and the supervisor switches the process's role between them
//! (`supervisor`). Every step takes the journal and answers a typed failure.

use std::path::{Path, PathBuf};
use std::time::Duration;

use herdr_core::node_migration::{self, IdTable, KnownProject, OwnerChange, copy};
use hide_node::ssh::transfer::FileCopy;
use hide_node::ssh::upstream::Upstream;
use hide_node::ssh::{SshAlias, shell_quote};
use serde_json::{Value, json};

use super::journal::{Journal, MoveFailure, Peer};

/// The most a step on the other machine may print.
const STEP_OUTPUT_CAP: usize = 64 * 1024;
/// How long one step on the other machine may take; starting its core
/// waits up to 30 s for it to take links.
const STEP_TIMEOUT: Duration = Duration::from_secs(60);

/// The other machine, reached over one SSH connection for the move.
pub struct Remote {
    upstream: Upstream,
    program: String,
    state_dir: Option<String>,
}

impl Remote {
    /// Dials nothing yet: the first step does.
    pub fn new(home: &Path, alias: &str, program: &str) -> Result<Self, MoveFailure> {
        let alias =
            SshAlias::from_config_file(&home.join(".ssh/config"), alias).map_err(|error| {
                MoveFailure::Unreachable {
                    reason: format!("ssh_alias: {}", error.diagnostic().reason),
                }
            })?;
        let upstream = Upstream::new(alias).map_err(|error| MoveFailure::Unreachable {
            reason: error.to_string(),
        })?;
        Ok(Self {
            upstream,
            program: program.to_owned(),
            state_dir: None,
        })
    }

    pub fn with_state_dir(mut self, state_dir: &str) -> Self {
        self.state_dir = Some(state_dir.to_owned());
        self
    }

    /// Runs `hided core-move <step>` there and answers its JSON line; a
    /// refusal is `Refused`.
    pub fn step(&self, step: &str, args: &[(&str, &str)]) -> Result<Value, MoveFailure> {
        let mut command = format!("{} core-move {step}", shell_quote(&self.program));
        if let Some(state_dir) = &self.state_dir {
            command.push_str(&format!(" --state-dir {}", shell_quote(state_dir)));
        }
        for (flag, value) in args {
            command.push_str(&format!(" --{flag} {}", shell_quote(value)));
        }
        let output = self
            .upstream
            .exec("core-move-step", &command, STEP_OUTPUT_CAP, STEP_TIMEOUT)
            .map_err(|error| {
                if error.never_reached_server() || error.a_move_can_change() {
                    MoveFailure::Unreachable {
                        reason: error.to_string(),
                    }
                } else {
                    MoveFailure::Refused {
                        step: step.to_owned(),
                        reason: error.to_string(),
                    }
                }
            })?;
        let line = output.stdout.lines().last().unwrap_or_default();
        let answer: Value = serde_json::from_str(line).map_err(|_| MoveFailure::Refused {
            step: step.to_owned(),
            reason: format!(
                "exit {}: {}",
                output.exit_status,
                output.stderr.trim().chars().take(512).collect::<String>()
            ),
        })?;
        if let Some(refused) = answer.get("refused") {
            return Err(match refused.get("file").and_then(Value::as_str) {
                Some(file) if step == "verify" => MoveFailure::Load {
                    file: file.to_owned(),
                    reason: text(refused, "reason"),
                },
                _ => MoveFailure::Refused {
                    step: step.to_owned(),
                    reason: text(refused, "reason"),
                },
            });
        }
        Ok(answer)
    }

    pub fn upload(
        &self,
        files: &[FileCopy],
        sent: &(dyn Fn(u64) + Sync),
    ) -> Result<(), MoveFailure> {
        self.upstream
            .upload(files, sent)
            .map_err(|error| MoveFailure::Copy {
                reason: error.to_string(),
            })
    }

    pub fn download(
        &self,
        files: &[FileCopy],
        received: &(dyn Fn(u64) + Sync),
    ) -> Result<(), MoveFailure> {
        self.upstream
            .download(files, received)
            .map_err(|error| MoveFailure::Copy {
                reason: error.to_string(),
            })
    }

    pub fn close(&self) {
        self.upstream.close();
    }
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// What the other machine says of itself before anything moves.
pub struct Inspected {
    pub node: String,
    pub state_dir: String,
    pub brain: Vec<String>,
    pub handover: Option<super::handover::Handover>,
    /// The Herdr socket its core would own; none when it finds no Herdr.
    pub herdr_socket: Option<String>,
}

pub fn inspect(remote: &Remote) -> Result<Inspected, MoveFailure> {
    let answer = remote.step("inspect", &[])?;
    let refused = |reason: &str| MoveFailure::Refused {
        step: "inspect".to_owned(),
        reason: reason.to_owned(),
    };
    Ok(Inspected {
        node: answer["node"]
            .as_str()
            .ok_or_else(|| refused("no node"))?
            .to_owned(),
        state_dir: answer["state_dir"]
            .as_str()
            .ok_or_else(|| refused("no state folder"))?
            .to_owned(),
        brain: serde_json::from_value(answer["brain"].clone())
            .map_err(|_| refused("no brain list"))?,
        handover: serde_json::from_value(answer["handover"].clone())
            .map_err(|_| refused("an unreadable handover"))?,
        herdr_socket: answer["herdr_socket"].as_str().map(str::to_owned),
    })
}

/// The owner change of a forward move from this machine to the device
/// `source` names, and the id table from every project both machines
/// hold. This machine's project roots are resolved here, as its node will
/// report them to the new core.
pub fn forward_change(
    source: &herdr_core::MoveSource,
    home: &Path,
    own_label: &str,
    own_herdr_socket: String,
    target_herdr_socket: String,
) -> Result<(OwnerChange, IdTable), MoveFailure> {
    let registration = serde_json::from_value(json!({
        "id": source.node,
        "label": own_label,
        "inbound": true,
    }))
    .map_err(|error| MoveFailure::Local {
        reason: error.to_string(),
    })?;
    let change = OwnerChange {
        old_owner: source.node.clone(),
        old_owner_as: source.node.clone(),
        new_owner: source.device_node.clone(),
        new_owner_was: source.device.clone(),
        old_owner_registration: registration,
        old_owner_herdr_socket: own_herdr_socket,
        new_owner_herdr_socket: target_herdr_socket,
    };
    let roots = project_roots(&source.projects, &source.node, home);
    let projects = source
        .projects
        .iter()
        .zip(&roots)
        .map(|(project, root)| KnownProject {
            id: &project.id,
            device_id: &project.device_id,
            path: &project.path,
            root,
            checkouts: project
                .checkouts
                .iter()
                .map(|(id, path)| (id.as_str(), path.as_str()))
                .collect(),
        });
    let ids = node_migration::id_table(&change, projects);
    Ok((change, ids))
}

/// The repository root each project's id digests: this machine's own
/// projects resolved on its disk, another machine's by the path it
/// reported as its root.
pub fn project_roots(
    projects: &[herdr_core::MoveProject],
    own_node: &str,
    home: &Path,
) -> Vec<PathBuf> {
    projects
        .iter()
        .map(|project| {
            if project.device_id != own_node {
                return PathBuf::from(&project.path);
            }
            match hide_host::register::check(Path::new(&project.path), home) {
                Ok(registrable) => PathBuf::from(registrable.root),
                // A folder that is gone is grouped by its own path, as this
                // machine's node answers for it.
                Err(error) => {
                    herdr_core::diagnostic!(json!({
                        "component": "core_move",
                        "kind": "staging.root_unresolved",
                        "project": project.id,
                        "reason": error.to_string(),
                    }));
                    PathBuf::from(&project.path)
                }
            }
        })
        .collect()
}

/// Copies this machine's brain state into its staging folder with the
/// device's label records beside it, and changes the copy's owner.
pub fn stage(
    state_dir: &Path,
    journal: &Journal,
    device_labels: &Value,
) -> Result<copy::Manifest, MoveFailure> {
    let staging = node_migration::staging_dir(state_dir, &journal.intent);
    let failed = |refusal: node_migration::Refusal| MoveFailure::Staging {
        file: refusal.file.display().to_string(),
        reason: refusal.reason,
    };
    copy::stage(state_dir, &staging).map_err(failed)?;
    carry_labels(&staging, &journal.change.new_owner_was, device_labels).map_err(|reason| {
        MoveFailure::Staging {
            file: staging.join("labels.json").display().to_string(),
            reason,
        }
    })?;
    let outcome = node_migration::reown(&staging, &journal.change, &journal.ids).map_err(failed)?;
    herdr_core::diagnostic!(json!({
        "component": "core_move",
        "kind": "staging.reowned",
        "intent": journal.intent,
        "files": outcome.files,
        "pruned": outcome.pruned,
    }));
    copy::digest(&staging).map_err(failed)
}

/// Writes the device's in-memory label records into the copy's
/// `labels.json` as the section `reown` makes the new owner's own.
pub(crate) fn carry_labels(staging: &Path, device: &str, records: &Value) -> Result<(), String> {
    let empty = records.as_object().is_none_or(serde_json::Map::is_empty);
    let path = staging.join("labels.json");
    let mut labels: Value = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if empty {
                return Ok(());
            }
            json!({"version": 1, "targets": {}})
        }
        Err(error) => return Err(error.to_string()),
    };
    if empty {
        return Ok(());
    }
    let object = labels
        .as_object_mut()
        .ok_or("labels.json is not an object")?;
    let moved = object
        .entry("moved")
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or("labels.json moved is not a map")?;
    moved.insert(format!("device:{device}"), records.clone());
    let bytes = serde_json::to_vec(&labels).map_err(|error| error.to_string())?;
    hide_platform::fs::atomic::write_file(&path, &bytes, hide_platform::fs::Access::Private)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Sends the staged copy and its manifest into the peer's
/// `move-incoming/<intent>`, then has the peer compare it; what differs is
/// sent once more. A copy the peer already holds from an earlier try is
/// not sent again.
pub fn send(
    remote: &Remote,
    state_dir: &Path,
    journal: &Journal,
    manifest: &copy::Manifest,
    progress: &(dyn Fn(u64, u64) + Sync),
) -> Result<(), MoveFailure> {
    let staging = node_migration::staging_dir(state_dir, &journal.intent);
    let incoming = format!(
        "{}/move-incoming/{}",
        journal.peer.state_dir.trim_end_matches('/'),
        journal.intent
    );
    let manifest_file = staging.with_extension("manifest.json");
    let bytes = serde_json::to_vec(manifest).map_err(|error| MoveFailure::Local {
        reason: error.to_string(),
    })?;
    hide_platform::fs::atomic::write_file(
        &manifest_file,
        &bytes,
        hide_platform::fs::Access::Private,
    )
    .map_err(|error| MoveFailure::Local {
        reason: error.to_string(),
    })?;
    let manifest_upload = FileCopy {
        local: manifest_file.clone(),
        remote: format!("{incoming}.manifest.json"),
    };
    remote.upload(std::slice::from_ref(&manifest_upload), &|_| {})?;
    let _ = std::fs::remove_file(&manifest_file);
    let total = manifest.total_bytes();
    let sent = std::sync::atomic::AtomicU64::new(0);
    let mut wanted = differing(remote, journal)?;
    let held = manifest.files.len() - wanted.len();
    let mut uploaded: Vec<String> = Vec::new();
    for attempt in 0..2 {
        if wanted.is_empty() {
            herdr_core::diagnostic!(json!({
                "component": "core_move",
                "kind": "copy.sent",
                "intent": journal.intent,
                "held": held,
                "uploaded": uploaded,
            }));
            return Ok(());
        }
        uploaded.extend(wanted.iter().cloned());
        let files: Vec<FileCopy> = wanted
            .iter()
            .map(|path| FileCopy {
                local: staging.join(path),
                remote: format!("{incoming}/{path}"),
            })
            .collect();
        if attempt == 0 {
            // Files the peer already holds count as sent.
            let held: u64 = manifest
                .files
                .iter()
                .filter(|(path, _)| !wanted.contains(path))
                .map(|(_, file)| file.size)
                .sum();
            sent.store(held, std::sync::atomic::Ordering::Relaxed);
            progress(held, total);
        }
        remote.upload(&files, &|bytes| {
            let now = sent.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed) + bytes;
            progress(now.min(total), total);
        })?;
        wanted = differing(remote, journal)?;
    }
    if wanted.is_empty() {
        Ok(())
    } else {
        Err(MoveFailure::Digest { files: wanted })
    }
}

/// The files the peer's copy lacks or holds differently; none means the
/// peer loaded the copy with its build.
fn differing(remote: &Remote, journal: &Journal) -> Result<Vec<String>, MoveFailure> {
    let answer = remote.step("verify", &[("intent", &journal.intent)])?;
    if answer["loadable"] == true {
        return Ok(Vec::new());
    }
    let differs: Vec<String> =
        serde_json::from_value(answer["differs"].clone()).map_err(|_| MoveFailure::Refused {
            step: "verify".to_owned(),
            reason: "an unreadable answer".to_owned(),
        })?;
    if differs.is_empty() {
        // Only files the manifest does not name differ: they are not sent,
        // and the peer would place them.
        return Err(MoveFailure::Refused {
            step: "verify".to_owned(),
            reason: format!(
                "the copy holds files the move did not send: {}",
                answer["extra"]
            ),
        });
    }
    Ok(differs)
}

pub fn place(remote: &Remote, journal: &Journal) -> Result<(), MoveFailure> {
    remote
        .step(
            "place",
            &[
                ("intent", &journal.intent),
                ("source", &journal.change.old_owner),
                ("target", &journal.change.new_owner),
            ],
        )
        .map(|_| ())
}

pub fn start_target(remote: &Remote, journal: &Journal) -> Result<(), MoveFailure> {
    remote
        .step("start", &[("intent", &journal.intent)])
        .map(|_| ())
        .map_err(|failure| match failure {
            MoveFailure::Refused { reason, .. } => MoveFailure::NotStarted { reason },
            other => other,
        })
}

/// What the peer's handover says of the move once the link that carries
/// its intent may have been sent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetSays {
    Active,
    /// Pending, or no record: the move did not commit there.
    NotCommitted,
}

pub fn target_status(remote: &Remote, journal: &Journal) -> Result<TargetSays, MoveFailure> {
    let answer = remote.step("status", &[("intent", &journal.intent)])?;
    Ok(
        match answer["handover"]["state"]["state"]
            .as_str()
            .or(answer["handover"]["state"].as_str())
        {
            Some("active") => TargetSays::Active,
            _ => TargetSays::NotCommitted,
        },
    )
}

/// Stops the peer's pending core and takes its copy back into
/// `move-incoming`; answers whether the peer had committed after all.
pub fn abort_target(remote: &Remote, journal: &Journal) -> Result<TargetSays, MoveFailure> {
    let answer = remote.step("abort", &[("intent", &journal.intent)])?;
    Ok(if answer["state"] == "active" {
        TargetSays::Active
    } else {
        TargetSays::NotCommitted
    })
}

pub fn finish_target(remote: &Remote, journal: &Journal) -> Result<(), MoveFailure> {
    remote
        .step("finish", &[("intent", &journal.intent)])
        .map(|_| ())
}

/// The peer this machine moves its core to, from the core's facts and the
/// peer's own answer.
pub fn peer(source: &herdr_core::MoveSource, inspected: &Inspected) -> Peer {
    Peer {
        device: source.device.clone(),
        alias: source.ssh_alias.clone(),
        node: inspected.node.clone(),
        program: source.helper_path.clone(),
        state_dir: inspected.state_dir.clone(),
    }
}
