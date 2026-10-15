//! Delivery of the Project Settings protections to the machine that owns
//! each project (PRD agent-guardrails D-11, D-20, D-28, B3, B4, B21, B31).
//!
//! Every machine holds one policy file its agents' hooks read: the projects
//! registered on that machine with a protection on. The core derives what
//! the file must say from its settings and registrations
//! ([`Runtime::guardrail_policy`]) and has the machine's node write it
//! (`Call::GuardrailPolicy`): at start for its own machine, at each device
//! connection, and whenever a persisted change moves a machine's policy.
//!
//! A settings save that moves a machine's policy is delivered first and
//! saved after, so Saved means the machine has the policy: the record waits
//! in [`GuardrailsWork::awaiting`], outside the frame the state writer saves,
//! until its machine answers. A failed delivery fails the save, keeps the
//! confirmed record, and writes the confirmed policy once more.
//!
//! One worker per machine writes its policy one call at a time and computes
//! the policy when it takes the job, so a burst of changes is one write of
//! the newest state. Nothing here touches a machine; the node does.

use std::time::Duration;

use super::*;
use crate::node_access::NodeLink;
use crate::project_settings::{self as settings, BaseBranchChoice, WriteState};
use hide_agent_hooks::guardrails::{Policy, ProjectPolicy};

/// A policy write is one small file replaced in place.
const PUBLISH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Default)]
pub(super) struct GuardrailsWork {
    /// What each machine last confirmed it holds; absent when unknown.
    published: BTreeMap<String, Policy>,
    /// Machines whose policy is to be written by their worker.
    queued: BTreeSet<String>,
    /// Machines whose worker runs.
    running: BTreeSet<String>,
    /// Saves waiting for their machine to take the policy they move, by
    /// settings id. Bounded with the pending saves by `settings::LIMIT`.
    pub(super) awaiting: BTreeMap<String, super::project_settings::Pending>,
}

/// One write a machine's worker makes.
pub(crate) struct Publication {
    link: Result<Arc<dyn NodeLink>, &'static str>,
    policy: Policy,
    /// The awaiting saves this policy carries.
    saves: Vec<String>,
}

#[cfg(test)]
impl Publication {
    pub(crate) fn policy(&self) -> &Policy {
        &self.policy
    }
}

impl Runtime {
    /// What `device`'s policy file must say: every project registered there
    /// with a protection on, by its confirmed record, a save on its way to
    /// disk, or a save waiting for this delivery.
    fn guardrail_policy(&self, device: &str) -> (Policy, Vec<String>) {
        let mut records: BTreeMap<&String, &settings::ProjectSettings> = self
            .snapshot
            .ui_state
            .project_settings
            .iter()
            .filter(|(_, record)| record.device_id == device)
            .collect();
        for (id, pending) in &self.project_settings_pending {
            if pending.record.device_id == device {
                records.insert(id, &pending.record);
            }
        }
        let mut saves = Vec::new();
        for (id, pending) in &self.guardrails.awaiting {
            if pending.record.device_id == device {
                records.insert(id, &pending.record);
                saves.push(id.clone());
            }
        }
        let projects = records
            .into_values()
            .filter(|record| self.guarded_registration(device, &record.root))
            .filter_map(|record| {
                let rules = record.rules()?;
                Some(ProjectPolicy {
                    root: record.root.clone(),
                    base: match &record.base_branch {
                        Some(BaseBranchChoice::Auto) => None,
                        Some(BaseBranchChoice::Branch { branch }) => Some(branch.clone()),
                        None => self
                            .snapshot
                            .ui_state
                            .project_base_branches
                            .get(&record.root)
                            .cloned(),
                    },
                    rules: rules.clone(),
                })
            })
            .collect();
        (Policy::new(projects), saves)
    }

    fn guarded_registration(&self, device: &str, root: &str) -> bool {
        self.snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .any(|registration| {
                registration.device_id == device && registration.path == root && !registration.home
            })
    }

    /// Whether `record`, saved for its machine, would change what that
    /// machine's policy says.
    pub(super) fn moves_guardrail_policy(
        &self,
        id: &str,
        record: &settings::ProjectSettings,
    ) -> bool {
        let rules = |record: Option<&settings::ProjectSettings>| {
            record
                .filter(|record| self.guarded_registration(&record.device_id, &record.root))
                .and_then(|record| {
                    record
                        .rules()
                        .map(|rules| (rules.clone(), record.base_branch.clone()))
                })
        };
        let before = self
            .project_settings_pending
            .get(id)
            .map(|pending| &pending.record)
            .or_else(|| self.snapshot.ui_state.project_settings.get(id));
        rules(before) != rules(Some(record))
    }

    /// Writes `device`'s policy whatever it last held: at start and at
    /// each connection, when the file may have changed or never been
    /// written (D-28).
    pub(super) fn publish_guardrail_policy(&mut self, device: &str) {
        if self.effects_held() {
            return;
        }
        self.guardrails.published.remove(device);
        self.guardrails.queued.insert(device.to_owned());
        self.start_guardrails_worker(device);
    }

    /// This machine's hooks read the protections as this core holds them,
    /// whatever an earlier run or an outside edit left in the file (D-28).
    /// Called once the core's workers can run.
    pub(crate) fn publish_own_guardrail_policy(&mut self) {
        let node = self.node.as_str().to_owned();
        self.publish_guardrail_policy(&node);
    }

    /// Queues a write for every machine whose policy no longer says what it
    /// last held. Called wherever settings or registrations are persisted,
    /// so a registration removed or a base branch chosen elsewhere reaches
    /// the hooks too (B11).
    pub(super) fn refresh_guardrail_policies(&mut self) {
        if self.effects_held() {
            return;
        }
        let mut devices: BTreeSet<String> = self.guardrails.published.keys().cloned().collect();
        devices.extend(
            self.snapshot
                .ui_state
                .project_settings
                .values()
                .chain(
                    self.project_settings_pending
                        .values()
                        .map(|pending| &pending.record),
                )
                .filter(|record| record.rules().is_some())
                .map(|record| record.device_id.clone()),
        );
        for device in devices {
            if self.guardrails.queued.contains(&device) {
                continue;
            }
            let (policy, _) = self.guardrail_policy(&device);
            if self.guardrails.published.get(&device) != Some(&policy) {
                self.guardrails.queued.insert(device.clone());
                self.start_guardrails_worker(&device);
            }
        }
    }

    /// A save whose record moves its machine's policy: it waits, reported
    /// as saving, until the machine has the policy (B3, B31).
    pub(super) fn await_guardrail_delivery(
        &mut self,
        id: String,
        pending: super::project_settings::Pending,
    ) {
        let device = pending.record.device_id.clone();
        self.guardrails.awaiting.insert(id, pending);
        self.guardrails.queued.insert(device.clone());
        self.start_guardrails_worker(&device);
    }

    fn start_guardrails_worker(&mut self, device: &str) {
        if self.guardrails.running.contains(device) {
            return;
        }
        let Some(context) = self.worker_context.clone() else {
            return;
        };
        self.guardrails.running.insert(device.to_owned());
        let runtime = context.runtime;
        let notifier = context.notifier;
        let worker_device = device.to_owned();
        let spawned = thread::Builder::new()
            .name("herdr-core-guardrails".into())
            .spawn(move || {
                loop {
                    let Some(core) = runtime.upgrade() else {
                        return;
                    };
                    let Ok(job) = core
                        .lock()
                        .map(|mut locked| locked.take_guardrail_publication(&worker_device))
                    else {
                        return;
                    };
                    drop(core);
                    let Some(job) = job else {
                        return;
                    };
                    let answer = job
                        .link
                        .as_ref()
                        .map_err(|reason| (*reason).to_owned())
                        .and_then(|link| {
                            crate::node_access::call_as::<serde_json::Value>(
                                link.as_ref(),
                                hide_node_link::protocol::Call::GuardrailPolicy {
                                    policy: job.policy.clone(),
                                },
                                PUBLISH_TIMEOUT,
                            )
                            .map(|_| ())
                            .map_err(|error| error.to_string())
                        });
                    let Some(core) = runtime.upgrade() else {
                        return;
                    };
                    let Ok(changed) = core.lock().map(|mut locked| {
                        locked.guardrail_policy_answered(&worker_device, job, answer)
                    }) else {
                        return;
                    };
                    drop(core);
                    if changed {
                        notifier.notify();
                    }
                }
            });
        if let Err(error) = spawned {
            self.guardrails.running.remove(device);
            crate::diagnostic!(serde_json::json!({
                "component": "guardrails",
                "kind": "worker.spawn_failed",
                "device_id": device,
                "error": error.to_string(),
            }));
            self.guardrails.queued.remove(device);
            let saves: Vec<String> = self
                .guardrails
                .awaiting
                .iter()
                .filter(|(_, pending)| pending.record.device_id == device)
                .map(|(id, _)| id.clone())
                .collect();
            for id in saves {
                self.fail_guardrail_save(&id);
            }
        }
    }

    /// The machine's next write, or `None` when nothing is queued, which
    /// ends its worker.
    pub(crate) fn take_guardrail_publication(&mut self, device: &str) -> Option<Publication> {
        if !self.guardrails.queued.remove(device) {
            self.guardrails.running.remove(device);
            return None;
        }
        let (policy, saves) = self.guardrail_policy(device);
        let link = if self.effects_held() {
            Err("effects_held")
        } else {
            self.linked_node(device).map(|(_, link)| link)
        };
        Some(Publication {
            link,
            policy,
            saves,
        })
    }

    /// Applies one write's answer; returns whether the screen changed.
    pub(crate) fn guardrail_policy_answered(
        &mut self,
        device: &str,
        job: Publication,
        answer: Result<(), String>,
    ) -> bool {
        match answer {
            Ok(()) => {
                crate::diagnostic!(serde_json::json!({
                    "component": "guardrails",
                    "kind": "policy.published",
                    "device_id": device,
                    "projects": job.policy.projects.len(),
                }));
                self.guardrails
                    .published
                    .insert(device.to_owned(), job.policy);
                let mut saved = false;
                for id in job.saves {
                    if let Some(pending) = self.guardrails.awaiting.remove(&id) {
                        self.save_project_settings(id, pending);
                        saved = true;
                    }
                }
                saved
            }
            Err(reason) => {
                crate::diagnostic!(serde_json::json!({
                    "component": "guardrails",
                    "kind": "policy.publish_failed",
                    "device_id": device,
                    "reason": reason,
                }));
                self.guardrails.published.remove(device);
                let mut failed = false;
                for id in job.saves {
                    failed |= self.fail_guardrail_save(&id);
                }
                // The node may have written before it failed to answer, so
                // the confirmed policy is written once more; a second
                // failure waits for the next start or connection.
                if failed {
                    self.guardrails.queued.insert(device.to_owned());
                    self.start_guardrails_worker(device);
                }
                failed
            }
        }
    }

    fn fail_guardrail_save(&mut self, id: &str) -> bool {
        let Some(pending) = self.guardrails.awaiting.remove(id) else {
            return false;
        };
        self.settle_project_settings(
            id,
            &pending,
            WriteState::Failed,
            Some("project_settings_policy_failed"),
        );
        true
    }

    /// A removed device's machine is no longer written.
    pub(super) fn forget_guardrails_device(&mut self, device: &str) {
        self.guardrails.published.remove(device);
        self.guardrails.queued.remove(device);
        let saves: Vec<String> = self
            .guardrails
            .awaiting
            .iter()
            .filter(|(_, pending)| pending.record.device_id == device)
            .map(|(id, _)| id.clone())
            .collect();
        for id in saves {
            self.fail_guardrail_save(&id);
        }
    }
}
