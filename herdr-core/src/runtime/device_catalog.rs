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
    /// The published session for a device: grouped by the facts known now,
    /// with the device's file tabs in its checkouts' strips.
    pub(super) fn derive_device_session(
        &self,
        target: &str,
        raw: &RemoteSessionSnapshot,
    ) -> RemoteSessionSnapshot {
        let empty = DeviceFacts::default();
        let facts = self.device_facts.get(target).unwrap_or(&empty);
        let mut session = device_catalog::group(target, raw, facts);
        join_device_editor_tabs(&mut session, &self.snapshot.editor.tabs);
        session
    }

    /// Recomputes a device's published session and catalog state from what
    /// is known now. Returns whether either changed.
    pub(super) fn refresh_device_catalog(&mut self, target: &str) -> bool {
        let Some(raw) = self.device_raw_sessions.get(target) else {
            return false;
        };
        let session = self.derive_device_session(target, raw);
        let empty = DeviceFacts::default();
        let catalog =
            device_catalog::catalog_state(raw, self.device_facts.get(target).unwrap_or(&empty));
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
        let missing = device_catalog::needed_paths(raw)
            .into_iter()
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
        }
        changed
    }

    /// A new helper connection answers afresh: a branch or a checkout may
    /// have moved while none was connected.
    pub(super) fn reset_device_facts(&mut self, target: &str) -> bool {
        self.device_facts.remove(target);
        self.request_device_facts(target)
    }

    pub(super) fn forget_device_catalog(&mut self, target: &str) {
        self.device_raw_sessions.remove(target);
        self.device_facts.remove(target);
    }
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
