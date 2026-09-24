//! A device's projects from its helper's facts (`crate::device_catalog`).
//!
//! The runtime keeps each device's session as Herdr reported it and derives
//! the published one from it and the facts the device's helper has answered.
//! The facts are asked on a worker with the runtime lock released, once per
//! directory for the life of a helper connection; a new directory in the
//! session, or a new helper connection, asks again.

use super::*;
use crate::device_catalog::{self, DeviceFacts, Fact};
use crate::host_access::{HostCallError, call_as};
use hide_project::ProjectFacts;
use std::time::Duration;

const FACTS_TIMEOUT: Duration = Duration::from_secs(15);

type FactAnswers = Vec<(String, Fact)>;

impl Runtime {
    /// The published session for a device, grouped by the facts known now;
    /// its strips are placed by `place_device_strips`.
    pub(super) fn derive_device_session(
        &self,
        target: &str,
        raw: &RemoteSessionSnapshot,
    ) -> RemoteSessionSnapshot {
        let empty = DeviceFacts::default();
        let facts = self.device_facts.get(target).unwrap_or(&empty);
        let mut session = device_catalog::group(target, raw, facts);
        device_catalog::apply_registrations(
            target,
            &mut session,
            &self.snapshot.ui_state.workspace_registrations,
            facts,
        );
        if let Some(worktrees) = self.device_worktrees.get(target) {
            for project in &mut session.workspaces {
                if let Some(listed) = worktrees.projects.get(&project.path) {
                    device_catalog::apply_worktrees(project, listed);
                }
            }
        }
        session
    }

    /// Recomputes a device's published session and catalog state from what
    /// is known now. Returns whether either changed.
    pub(super) fn refresh_device_catalog(&mut self, target: &str) -> bool {
        let Some(raw) = self.device_raw_sessions.get(target) else {
            return false;
        };
        let mut session = self.derive_device_session(target, raw);
        let empty = DeviceFacts::default();
        let catalog =
            device_catalog::catalog_state(raw, self.device_facts.get(target).unwrap_or(&empty));
        let mut dropped_moves = Vec::new();
        self.place_device_strips(target, &mut session, &mut dropped_moves);
        if !dropped_moves.is_empty() {
            self.report_dropped_tab_moves(dropped_moves);
        }
        let Some(status) = self
            .snapshot
            .status
            .remote
            .iter_mut()
            .find(|status| status.target_id == target)
        else {
            return false;
        };
        let mut changed = false;
        if status.catalog != catalog {
            status.catalog = catalog;
            changed = true;
        }
        if status.session.as_ref() != Some(&session) {
            status.session = Some(session);
            changed = true;
        }
        if changed {
            self.repoint_device_editor_tabs(target);
        }
        changed
    }

    /// A file tab names its checkout's project; grouping moves a checkout
    /// into a project, so its tabs follow it there.
    fn repoint_device_editor_tabs(&mut self, target: &str) {
        let Some(session) = self
            .snapshot
            .status
            .remote
            .iter()
            .find(|status| status.target_id == target)
            .and_then(|status| status.session.as_ref())
        else {
            return;
        };
        let owners = session
            .workspaces
            .iter()
            .flat_map(|workspace| {
                workspace
                    .checkouts
                    .iter()
                    .map(|checkout| (checkout.id.clone(), workspace.id.clone()))
            })
            .collect::<HashMap<_, _>>();
        for tab in &mut self.snapshot.editor.tabs {
            if let Some(owner) = owners.get(&tab.checkout_id)
                && &tab.workspace_id != owner
            {
                tab.workspace_id = owner.clone();
            }
        }
    }

    /// Asks the device's helper for the facts of every directory its session
    /// names that has no answer yet, unless a request is already running.
    pub(super) fn request_device_facts(&mut self, target: &str) -> bool {
        let Some(raw) = self.device_raw_sessions.get(target) else {
            return false;
        };
        let known = self.device_facts.get(target);
        // One request at a time; and a helper that could not be reached is
        // not asked again on every session sync, only when a connection is
        // established (`reset_device_facts`).
        if known.is_some_and(|facts| facts.in_flight.is_some() || facts.unavailable.is_some()) {
            return self.refresh_device_catalog(target);
        }
        let registered = self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .filter(|registration| registration.device_id == target)
            .map(|registration| registration.path.clone());
        let missing = device_catalog::needed_paths(raw)
            .into_iter()
            .chain(registered)
            .filter(|path| known.is_none_or(|facts| !facts.facts.contains_key(path)))
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return self.refresh_device_catalog(target);
        }
        if self.device_host_connecting(target) {
            return self.refresh_device_catalog(target);
        }
        let channel = match self.device_channel(target) {
            Ok(channel) => channel,
            Err(_) if self.device_host_connecting(target) => {
                return self.refresh_device_catalog(target);
            }
            Err(reason) => {
                self.device_facts
                    .entry(target.to_owned())
                    .or_default()
                    .unavailable = Some(reason);
                return self.refresh_device_catalog(target);
            }
        };
        let generation = self.device_host_generation(target);
        let entry = self.device_facts.entry(target.to_owned()).or_default();
        entry.in_flight = Some(generation);
        entry.unavailable = None;
        crate::diagnostic!(serde_json::json!({
            "component": "device_catalog",
            "kind": "catalog.facts_requested",
            "target": target,
            "generation": generation,
            "paths": missing.len(),
        }));
        let Some(context) = self.worker_context.clone() else {
            let (answers, failure) = ask_facts(channel.as_ref(), &missing);
            return self.ingest_device_facts(target, generation, answers, failure);
        };
        let device = target.to_owned();
        let spawned = thread::Builder::new()
            .name("herdr-core-device-catalog".to_owned())
            .spawn(move || {
                let (answers, failure) = ask_facts(channel.as_ref(), &missing);
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => {
                        guard.ingest_device_facts(&device, generation, answers, failure)
                    }
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawned {
            let entry = self.device_facts.entry(target.to_owned()).or_default();
            entry.in_flight = None;
            entry.unavailable = Some(format!("The catalog reader could not start: {error}"));
        }
        self.refresh_device_catalog(target)
    }

    /// The helper answered. An answer from a connection that has since been
    /// replaced is dropped; the new connection asks again.
    pub(super) fn ingest_device_facts(
        &mut self,
        target: &str,
        generation: u64,
        answers: FactAnswers,
        failure: Option<String>,
    ) -> bool {
        let Some(entry) = self.device_facts.get_mut(target) else {
            return false;
        };
        if entry.in_flight != Some(generation) {
            return false;
        }
        entry.in_flight = None;
        let answered = answers.len();
        for (path, fact) in answers {
            entry.facts.insert(path, fact);
        }
        entry.unavailable = failure.clone();
        crate::diagnostic!(serde_json::json!({
            "component": "device_catalog",
            "kind": "catalog.facts_answered",
            "target": target,
            "generation": generation,
            "answered": answered,
            "failure": failure,
        }));
        let mut changed = self.refresh_device_catalog(target);
        // The session may have named new directories while this ran.
        if failure.is_none() {
            changed |= self.request_device_facts(target);
            changed |= self.request_device_worktrees(target, false);
        }
        changed
    }

    /// Reads the worktrees of the device's Git repositories through its
    /// helper: every repository when `all` (after a worktree was created or
    /// removed there, or a new connection), otherwise the ones not read yet.
    /// One read at a time; a request during it runs one more after it.
    pub(super) fn request_device_worktrees(&mut self, target: &str, all: bool) -> bool {
        let Some(session) = self
            .snapshot
            .status
            .remote
            .iter()
            .find(|status| status.target_id == target)
            .and_then(|status| status.session.as_ref())
        else {
            return false;
        };
        let roots = device_catalog::git_roots(session);
        let entry = self.device_worktrees.entry(target.to_owned()).or_default();
        if entry.in_flight.is_some() {
            entry.again |= all || roots.iter().any(|root| !entry.projects.contains_key(root));
            return false;
        }
        let wanted = roots
            .into_iter()
            .filter(|root| all || !entry.projects.contains_key(root))
            .collect::<Vec<_>>();
        if wanted.is_empty() {
            return false;
        }
        let Ok(channel) = self.device_channel(target) else {
            return false;
        };
        let generation = self.device_host_generation(target);
        let entry = self.device_worktrees.entry(target.to_owned()).or_default();
        entry.in_flight = Some(generation);
        entry.again = false;
        let Some(context) = self.worker_context.clone() else {
            let (answers, failure) = ask_worktrees(channel.as_ref(), &wanted);
            return self.ingest_device_worktrees(target, generation, answers, failure);
        };
        let device = target.to_owned();
        let spawned = thread::Builder::new()
            .name("herdr-core-device-worktrees".to_owned())
            .spawn(move || {
                let (answers, failure) = ask_worktrees(channel.as_ref(), &wanted);
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => {
                        guard.ingest_device_worktrees(&device, generation, answers, failure)
                    }
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawned {
            let entry = self.device_worktrees.entry(target.to_owned()).or_default();
            entry.in_flight = None;
            entry.unavailable = Some(format!("The worktree reader could not start: {error}"));
        }
        false
    }

    /// The helper answered for the device's repositories. An answer from a
    /// connection that has since been replaced is dropped.
    pub(super) fn ingest_device_worktrees(
        &mut self,
        target: &str,
        generation: u64,
        answers: Vec<(String, Option<crate::model::ProjectWorktreesSnapshot>)>,
        failure: Option<String>,
    ) -> bool {
        let Some(entry) = self.device_worktrees.get_mut(target) else {
            return false;
        };
        if entry.in_flight != Some(generation) {
            return false;
        }
        entry.in_flight = None;
        for (root, project) in answers {
            match project {
                Some(project) => {
                    entry.projects.insert(root, project);
                }
                None => {
                    entry.projects.remove(&root);
                }
            }
        }
        if let Some(reason) = failure.as_deref() {
            crate::diagnostic!(serde_json::json!({
                "component": "device_worktrees",
                "kind": "worktrees.read_failed",
                "target": target,
                "generation": generation,
                "message": reason,
            }));
        }
        entry.unavailable = failure;
        let again = std::mem::take(&mut entry.again);
        let mut changed = self.refresh_device_catalog(target);
        if again {
            changed |= self.request_device_worktrees(target, true);
        }
        changed
    }

    /// A new helper connection answers afresh: a branch or a checkout may
    /// have moved while none was connected.
    pub(super) fn reset_device_facts(&mut self, target: &str) -> bool {
        self.device_facts.remove(target);
        self.device_worktrees.remove(target);
        self.request_device_facts(target)
    }

    pub(super) fn forget_device_catalog(&mut self, target: &str) {
        self.device_raw_sessions.remove(target);
        self.device_facts.remove(target);
        self.device_worktrees.remove(target);
    }
}

const WORKTREES_TIMEOUT: Duration = Duration::from_secs(60);

/// Reads each repository in turn; a refusal is that repository's answer (no
/// rows), a connection failure stops the batch with the reason.
fn ask_worktrees(
    channel: &dyn crate::host_access::HostChannel,
    roots: &[String],
) -> (
    Vec<(String, Option<crate::model::ProjectWorktreesSnapshot>)>,
    Option<String>,
) {
    let mut answers = Vec::new();
    for root in roots {
        match call_as::<Option<hide_host::worktrees::RepositoryWorktrees>>(
            channel,
            hide_host::protocol::Call::Worktrees {
                path: root.clone(),
                bases: Default::default(),
                base_override: None,
            },
            WORKTREES_TIMEOUT,
        ) {
            Ok(facts) => {
                answers.push((root.clone(), facts.map(crate::worktrees::project_snapshot)))
            }
            Err(HostCallError::Refused(error)) => {
                answers.push((
                    root.clone(),
                    Some(crate::model::ProjectWorktreesSnapshot {
                        root_path: root.clone(),
                        unavailable_reason: Some(error.message),
                        ..Default::default()
                    }),
                ));
            }
            Err(error) => return (answers, Some(error.to_string())),
        }
    }
    (answers, None)
}

/// Asks for each directory in turn. A refusal is that directory's answer; a
/// connection failure stops the batch, and the directories not reached stay
/// unanswered with the reason.
fn ask_facts(
    channel: &dyn crate::host_access::HostChannel,
    paths: &[String],
) -> (FactAnswers, Option<String>) {
    let mut answers = Vec::new();
    for path in paths {
        match call_as::<ProjectFacts>(
            channel,
            hide_host::protocol::Call::Project { path: path.clone() },
            FACTS_TIMEOUT,
        ) {
            Ok(facts) => answers.push((path.clone(), Fact::Known(facts))),
            Err(HostCallError::Refused(error)) => {
                answers.push((path.clone(), Fact::Refused(error.message)));
            }
            Err(error) => return (answers, Some(error.to_string())),
        }
    }
    (answers, None)
}

const REGISTRABLE_TIMEOUT: Duration = Duration::from_secs(15);

impl Runtime {
    /// `create_workspace` on a device (B23, B24): the device's helper judges
    /// the folder against that device's home and names the project it
    /// belongs to; the registration is that project, and it is listed with
    /// or without a Herdr workspace there. Nothing is created on the device.
    pub(super) fn create_device_registration(
        &mut self,
        device: &str,
        path: String,
        label: String,
    ) -> bool {
        let channel = match self.device_channel(device) {
            Ok(channel) => channel,
            Err(message) => {
                self.set_error(
                    "workspace.create_unavailable",
                    format!("Adding a project on this device needs its connection: {message}"),
                    true,
                );
                return true;
            }
        };
        let ask = move || {
            call_as::<hide_host::register::Registrable>(
                channel.as_ref(),
                hide_host::protocol::Call::Registrable { path: path.clone() },
                REGISTRABLE_TIMEOUT,
            )
            .map_err(|error| match error {
                HostCallError::Refused(error) => error.message,
                other => other.to_string(),
            })
        };
        let Some(context) = self.worker_context.clone() else {
            let answer = ask();
            return self.ingest_device_registration(device, label, answer);
        };
        let device = device.to_owned();
        let spawned = thread::Builder::new()
            .name("herdr-core-device-registration".to_owned())
            .spawn(move || {
                let answer = ask();
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_device_registration(&device, label, answer),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawned {
            self.set_error(
                "workspace.create_worker_failed",
                format!("The registration worker could not be started: {error}"),
                true,
            );
        }
        true
    }

    pub(super) fn ingest_device_registration(
        &mut self,
        device: &str,
        label: String,
        answer: Result<hide_host::register::Registrable, String>,
    ) -> bool {
        // A device removed while its helper judged the folder takes no
        // registration from that late answer.
        // Removing a device forgets its host (`forget_device_host`).
        if !self.device_hosts.contains_key(device) {
            crate::diagnostic!(serde_json::json!({
                "component": "registration", "kind": "create.device_gone",
                "target": device,
            }));
            return false;
        }
        let registrable = match answer {
            Ok(registrable) => registrable,
            Err(message) => {
                crate::diagnostic!(serde_json::json!({
                    "component": "registration", "kind": "create.refused",
                    "target": device, "message": message,
                }));
                self.set_error("workspace.create_refused", message, false);
                return true;
            }
        };
        let id = device_catalog::project_id(device, Path::new(&registrable.root));
        if self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .any(|registration| registration.id == id)
        {
            // Registering the same project again reaches the same state.
            return false;
        }
        let label = Some(label.trim())
            .filter(|label| !label.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                Path::new(&registrable.root)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| registrable.root.clone());
        self.snapshot
            .ui_state
            .workspace_registrations
            .push(crate::model::WorkspaceRegistration {
                id: id.clone(),
                label,
                path: registrable.root,
                device_id: device.to_owned(),
                pinned: false,
            });
        self.persist_ui_state();
        crate::diagnostic!(serde_json::json!({
            "component": "registration", "kind": "workspace.registered",
            "target": device, "workspace_id": id,
        }));
        self.request_device_facts(device);
        self.refresh_device_catalog(device);
        true
    }
}
