//! Each machine's install kit as the runtime holds it (PRD device-parity):
//! what the last check of each machine found, the kit work each machine runs
//! next, and the operator's Reinstall.
//!
//! This Mac's kit runs on its own worker (`crate::kit::KitPump`); a device's
//! runs on its helper, called from a worker that exists while that device
//! has work queued (`crate::kit::spawn_device_worker`). Nothing here touches
//! a file or a process: a worker takes its job under the lock, runs it with
//! the lock released, and hands the report back.

use std::sync::Arc;
use std::time::{Duration, Instant};

use hide_kit::{ComponentId, KitReport, Scope};

use super::Runtime;
use super::hosts::HostPhase;
use crate::host_access::HostChannel;
use crate::model::KitSnapshot;
use crate::workspace::LOCAL_DEVICE_ID;

/// How often this Mac's kit is read again while Settings is on screen, so a
/// part the operator removed by hand shows up without a relaunch.
const LOCAL_STATUS_INTERVAL: Duration = Duration::from_secs(5);

/// What a machine's kit worker is asked to do next.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum KitJob {
    Apply(Scope),
    Status,
}

impl KitJob {
    /// Two requests for one machine as one: an install answers a read too,
    /// and a Reinstall does everything the connection pass does.
    fn merge(self, later: KitJob) -> KitJob {
        match (self, later) {
            (KitJob::Apply(before), KitJob::Apply(after)) => {
                KitJob::Apply(merge_scopes(Some(before), after))
            }
            (KitJob::Apply(scope), KitJob::Status) | (KitJob::Status, KitJob::Apply(scope)) => {
                KitJob::Apply(scope)
            }
            (KitJob::Status, KitJob::Status) => KitJob::Status,
        }
    }
}

fn merge_scopes(before: Option<Scope>, after: Scope) -> Scope {
    match before {
        Some(before) => before.merge(after),
        None => after,
    }
}

/// What a device's kit worker does on one call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DeviceKitWork {
    Job(KitJob),
    /// The device is being removed from Hide: its kit comes off (D-16).
    Remove,
}

/// One kit call a device's worker makes, with what it needs from the
/// runtime taken under the lock.
pub(crate) struct DeviceKitCall {
    pub(crate) work: DeviceKitWork,
    pub(crate) channel: Arc<dyn HostChannel>,
    pub(crate) cli_dir: String,
    pub(crate) herdr_socket: Option<String>,
    pub(crate) retirement_projects: Vec<String>,
}

/// What a device's kit call answered.
pub(crate) enum DeviceKitAnswer {
    Report(Result<KitReport, String>),
    Removed(Result<hide_host::protocol::KitRemoved, String>),
}

/// Where one device stands with the first-run agent choice in this run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FirstRunChoice {
    /// Its record waits for the choice and this run has not sent it.
    Waiting,
    /// This run sent it; a device that still waits after that is reported.
    Sent,
}

/// Why Hide's kit does not run on a device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum KitDeclined {
    /// The operator has not allowed Hide's helper there.
    NoConsent,
    /// The consent was given to an older build's kit; Hide asks again. What
    /// that build put there may still be in place.
    ConsentOutdated,
    /// This build has no helper for the device's platform.
    Unsupported(String),
}

impl KitDeclined {
    fn reason(self) -> String {
        match self {
            Self::NoConsent => {
                "Hide installs its kit here once you allow its helper on this device".to_owned()
            }
            Self::ConsentOutdated => {
                "Hide needs your permission again before it installs its kit here".to_owned()
            }
            Self::Unsupported(message) => message,
        }
    }

    /// Whether Hide has certainly put no hook there: never allowed, or a
    /// platform it has no helper for. An outdated consent may leave an
    /// older build's hooks in place, so that answer stays unknown.
    pub(super) fn installed_nothing(&self) -> bool {
        !matches!(self, Self::ConsentOutdated)
    }
}

/// What a removal leaves on a device whose helper is not connected (B24).
const LEFT_ON_DEVICE: [&str; 3] = ["Hide's hook entries", "Hide's hide link", "the helper root"];

impl Runtime {
    /// Registered roots and the catalog's known checkouts, on this device only.
    /// This copies paths; the kit inspects their run states outside the lock.
    pub(crate) fn retirement_projects(&self, device_id: &str) -> Vec<String> {
        let mut paths = self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .filter(|registration| registration.device_id == device_id)
            .map(|registration| registration.path.clone())
            .collect::<Vec<_>>();
        let projects = self.snapshot.navigator.workspaces.iter().chain(
            self.snapshot
                .status
                .remote
                .iter()
                .filter(|status| status.target_id == device_id)
                .filter_map(|status| status.session.as_ref())
                .flat_map(|session| session.workspaces.iter()),
        );
        for project in
            projects.filter(|project| project.device_id == device_id && project.registered)
        {
            paths.extend(
                project
                    .checkouts
                    .iter()
                    .map(|checkout| checkout.path.clone()),
            );
        }
        paths.sort();
        paths.dedup();
        paths
    }

    /// This Mac cannot run the kit at all, and its row says why (B11).
    pub(crate) fn set_local_kit_unavailable(&mut self, reason: &str) {
        self.set_kit_state(LOCAL_DEVICE_ID, KitSnapshot::unavailable(reason));
    }

    /// The worker exists: the launch pass is its first job (B1, B10).
    pub(crate) fn queue_local_kit_launch(&mut self) {
        self.local_kit_pending = Some(Scope::automatic());
        self.set_kit_busy(LOCAL_DEVICE_ID);
    }

    /// The next job for this Mac's kit worker: a queued install first, else
    /// a re-read while Settings is on screen; off screen nothing is read.
    pub(crate) fn take_local_kit_job(&mut self, now: Instant) -> Option<KitJob> {
        if let Some(scope) = self.local_kit_pending.take() {
            return Some(KitJob::Apply(scope));
        }
        if std::mem::take(&mut self.local_kit_check_requested)
            || self.settings_observed() && self.local_kit_next_status.is_none_or(|next| now >= next)
        {
            self.local_kit_next_status = Some(now + LOCAL_STATUS_INTERVAL);
            return Some(KitJob::Status);
        }
        None
    }

    /// A Settings tab showing the kit opened: this Mac and every connected
    /// device are read once. A machine that cannot run the kit has nothing
    /// to read, and one with work queued answers with that work.
    pub(super) fn request_kit_check(&mut self) -> bool {
        if self
            .kit_states
            .get(LOCAL_DEVICE_ID)
            .is_none_or(|state| state.unavailable.is_none())
        {
            self.local_kit_check_requested = true;
        }
        let ready = self
            .device_hosts
            .iter()
            .filter(|(_, host)| matches!(host.phase, HostPhase::Ready { .. }))
            .map(|(device_id, _)| device_id.clone())
            .collect::<Vec<_>>();
        for device_id in ready {
            if !self.device_kit_pending.contains_key(&device_id)
                && !self.device_kit_running.contains(&device_id)
            {
                self.queue_device_kit(&device_id, KitJob::Status);
            }
        }
        false
    }

    /// Stores what a check or an install found on one machine. A report that
    /// lands while an install is queued for that machine keeps it busy.
    pub(crate) fn ingest_kit_report(&mut self, device_id: &str, report: &KitReport) -> bool {
        // The retired labels plugin is no part the operator acts on; what was
        // taken out, and what stayed for the next pass, goes to the log
        // (PRD labels-in-hided D-12, design principle 13).
        let retirement = &report.labels_retirement;
        if !retirement.is_empty() {
            crate::diagnostic!(serde_json::json!({
                "component": "labels",
                "kind": if retirement.failures.is_empty() { "plugin.retired" } else { "plugin.retire_incomplete" },
                "device_id": device_id,
                "removed": retirement.removed,
                "failures": retirement.failures,
            }));
        }
        // What an older layout left (the standalone hcoord plugin, folders
        // `~/.hide` replaced) is no part either (PRD hide-home-layout D-13).
        let legacy = &report.legacy_retirement;
        if !legacy.is_empty() {
            crate::diagnostic!(serde_json::json!({
                "component": "kit",
                "kind": if legacy.failures.is_empty() { "legacy.retired" } else { "legacy.retire_incomplete" },
                "device_id": device_id,
                "removed": legacy.removed,
                "failures": legacy.failures,
            }));
        }
        let mut changed = self.decide_agent_onboarding(device_id, report);
        let mut snapshot = KitSnapshot::from_report(report);
        snapshot.busy = self.kit_install_queued(device_id);
        changed |= self.set_kit_state(device_id, snapshot);
        changed
    }

    /// What a machine's kit report says about the first-run agent choice.
    ///
    /// The kit's record is the one place that knows whether a machine was
    /// asked and answered (`held_for_onboarding`); `ui_state.agent_onboarding`
    /// follows it for this Mac and keeps the answer given, which is the choice
    /// a device that connects later receives. This Mac's record waiting asks;
    /// a record that never waited is an existing install, which is never
    /// asked and keeps what it has on as the saved choice. A device whose own
    /// record waits, once this Mac has answered, gets that choice sent once
    /// per run by its own detection; before that nothing is installed there.
    fn decide_agent_onboarding(&mut self, device_id: &str, report: &KitReport) -> bool {
        use crate::model::AgentOnboarding;
        // A pass that could not run (retirement refused, account lock) lists
        // no agents and says nothing about the record, so it decides nothing.
        if report.agents.is_empty() {
            return false;
        }
        if device_id != LOCAL_DEVICE_ID {
            if !report.held_for_onboarding {
                self.device_first_run_choice.remove(device_id);
                return false;
            }
            self.device_first_run_choice
                .entry(device_id.to_owned())
                .or_insert(FirstRunChoice::Waiting);
            if self.snapshot.ui_state.agent_onboarding == Some(AgentOnboarding::Done) {
                self.send_first_run_choice(device_id);
            }
            return false;
        }
        let before = self.snapshot.ui_state.agent_onboarding;
        let after = if report.held_for_onboarding {
            // An Apply just pressed has its install queued here: a report that
            // was already on its way says the old thing and must not bring
            // the question back for a moment.
            if before == Some(AgentOnboarding::Done) && self.local_kit_pending.is_some() {
                return false;
            }
            AgentOnboarding::Pending
        } else {
            AgentOnboarding::Done
        };
        if before == Some(after) {
            return false;
        }
        self.snapshot.ui_state.agent_onboarding = Some(after);
        if after == AgentOnboarding::Done && before != Some(AgentOnboarding::Done) {
            // Never asked, or answered outside this app: what it has on today
            // is the choice a device added later receives, so that device
            // gets no less than before the choice existed.
            self.snapshot.ui_state.agent_onboarding_agents = report
                .agents
                .iter()
                .filter(|agent| agent.enabled)
                .map(|agent| agent.id.clone())
                .collect();
            self.send_first_run_choice_to_waiting_devices();
        }
        self.persist_ui_state();
        true
    }

    /// Sends the saved choice to a device that waits for it, once per run;
    /// a second report from the same device that still waits is a failure
    /// to answer, logged and not retried (engineering rules 11 and 15).
    fn send_first_run_choice(&mut self, device_id: &str) {
        if self.device_first_run_choice.get(device_id) == Some(&FirstRunChoice::Sent) {
            crate::diagnostic!(serde_json::json!({
                "component": "kit",
                "kind": "first_run_choice.unanswered",
                "device_id": device_id,
            }));
            return;
        }
        self.device_first_run_choice
            .insert(device_id.to_owned(), FirstRunChoice::Sent);
        let agents = self.snapshot.ui_state.agent_onboarding_agents.clone();
        self.queue_device_kit(
            device_id,
            KitJob::Apply(Scope::first_run(agents.iter().map(String::as_str))),
        );
    }

    /// A choice that was queued for a device and did not run is unsent again.
    fn reopen_first_run_choice(&mut self, device_id: &str) {
        if let Some(state) = self.device_first_run_choice.get_mut(device_id) {
            *state = FirstRunChoice::Waiting;
        }
    }

    fn send_first_run_choice_to_waiting_devices(&mut self) {
        let waiting = self
            .device_first_run_choice
            .iter()
            .filter(|(_, state)| **state == FirstRunChoice::Waiting)
            .map(|(device_id, _)| device_id.clone())
            .collect::<Vec<_>>();
        for device_id in waiting {
            self.send_first_run_choice(&device_id);
        }
    }

    /// The operator applied the first-run choice: the agents left on are
    /// switched on here and on every device that waits for it, each by its
    /// own detection, and the choice is kept for devices that connect later.
    pub(super) fn apply_agent_onboarding(&mut self, agents: Vec<String>) -> bool {
        if let Some(unknown) = agents
            .iter()
            .find(|agent| hide_kit::agents::adapter(agent).is_none())
        {
            self.set_error(
                "kit.unknown_agent",
                format!("{unknown} is not an agent Hide knows"),
                false,
            );
            return true;
        }
        let mut agents = agents;
        agents.sort();
        agents.dedup();
        self.finish_agent_onboarding(agents)
    }

    /// Ends the first-run choice with `agents` on; only a pending choice can
    /// end, so a second Apply or a stale client changes nothing
    /// (engineering rule 11). The choice reads as made at once; this Mac's
    /// record confirms it, and says otherwise (the question comes back) when
    /// it could not be saved.
    pub(super) fn finish_agent_onboarding(&mut self, agents: Vec<String>) -> bool {
        use crate::model::AgentOnboarding;
        if self.snapshot.ui_state.agent_onboarding != Some(AgentOnboarding::Pending) {
            return false;
        }
        self.snapshot.ui_state.agent_onboarding = Some(AgentOnboarding::Done);
        self.snapshot.ui_state.agent_onboarding_agents = agents.clone();
        self.persist_ui_state();
        self.queue_kit_scope(
            LOCAL_DEVICE_ID,
            Scope::first_run(agents.iter().map(String::as_str)),
        );
        self.send_first_run_choice_to_waiting_devices();
        true
    }

    fn kit_install_queued(&self, device_id: &str) -> bool {
        if device_id == LOCAL_DEVICE_ID {
            self.local_kit_pending.is_some()
        } else {
            matches!(
                self.device_kit_pending.get(device_id),
                Some(KitJob::Apply(_))
            )
        }
    }

    /// The operator pressed Reinstall on a machine's row: every part that is
    /// outdated, missing, removed or failed is installed again, and the parts
    /// in place are not touched (B8, B9).
    pub(super) fn request_kit_reinstall(
        &mut self,
        device_id: &str,
        only: Option<&[ComponentId]>,
        only_agents: Option<&[String]>,
    ) -> bool {
        if device_id != LOCAL_DEVICE_ID && !self.device_registration_exists(device_id) {
            self.set_error(
                "kit.unknown_machine",
                format!("Device {device_id} is not registered"),
                false,
            );
            return true;
        }
        let state = self.kit_view(device_id);
        if let Some(reason) = state.unavailable {
            self.set_error("kit.unavailable", reason, false);
            return true;
        }
        // Naming parts or agents narrows the repair to what is named.
        let named = only.is_some() || only_agents.is_some();
        let parts = state
            .components
            .iter()
            .filter(|part| part.state.needs_attention())
            .filter(|part| match only {
                Some(only) => only.contains(&part.id),
                None => !named,
            })
            .map(|part| part.id)
            .collect::<Vec<_>>();
        let agents = state
            .agents
            .iter()
            .filter(|agent| agent.needs_attention())
            .filter(|agent| match only_agents {
                Some(only) => only.contains(&agent.id),
                None => !named,
            })
            .map(|agent| agent.id.as_str())
            .collect::<Vec<_>>();
        // A second press after the first one repaired everything is the same
        // intent, already met (engineering rule 11).
        if parts.is_empty() && agents.is_empty() {
            return false;
        }
        let scope = Scope::reinstall(parts).merge(Scope::agents(agents, []));
        self.queue_kit_scope(device_id, scope)
    }

    /// The operator switched an agent on or off from its row (issue #517).
    /// An agent already where the switch puts it, or that is not set up on
    /// that machine, is the same intent, already met (engineering rule 11).
    pub(super) fn request_kit_agent_set(
        &mut self,
        device_id: &str,
        agent: &str,
        enabled: bool,
    ) -> bool {
        if hide_kit::agents::adapter(agent).is_none() {
            self.set_error(
                "kit.unknown_agent",
                format!("{agent} is not an agent Hide knows"),
                false,
            );
            return true;
        }
        if device_id != LOCAL_DEVICE_ID && !self.device_registration_exists(device_id) {
            self.set_error(
                "kit.unknown_machine",
                format!("Device {device_id} is not registered"),
                false,
            );
            return true;
        }
        let state = self.kit_view(device_id);
        if let Some(reason) = state.unavailable {
            self.set_error("kit.unavailable", reason, false);
            return true;
        }
        let Some(now) = state.agents.iter().find(|row| row.id == agent) else {
            return false;
        };
        // The latest intent wins (engineering rule 11): what is queued for
        // this agent decides, and while an install runs the last report is
        // older than that install, so it decides nothing.
        let already = match self.queued_agent_choice(device_id, agent) {
            Some(queued) => queued == enabled,
            None if state.busy => false,
            None if enabled => {
                // Nothing to switch on where the agent is not set up; an agent
                // that is on and has nothing to repair is met.
                now.availability != hide_kit::Availability::Available
                    || (now.enabled && !now.needs_attention())
            }
            None => !now.enabled,
        };
        if enabled && now.availability != hide_kit::Availability::Available {
            return false;
        }
        if already {
            return false;
        }
        let scope = if enabled {
            Scope::agents([agent], [])
        } else {
            Scope::agents([], [agent])
        };
        self.queue_kit_scope(device_id, scope)
    }

    /// What the work still queued for a machine says about one agent's
    /// switch: `Some(true)` on, `Some(false)` off, `None` when it says nothing.
    fn queued_agent_choice(&self, device_id: &str, agent: &str) -> Option<bool> {
        let scope = if device_id == LOCAL_DEVICE_ID {
            self.local_kit_pending.as_ref()
        } else {
            match self.device_kit_pending.get(device_id) {
                Some(KitJob::Apply(scope)) => Some(scope),
                _ => None,
            }
        }?;
        if scope.agent_off.contains(agent) {
            Some(false)
        } else if scope.agent_on.contains(agent) {
            Some(true)
        } else {
            None
        }
    }

    /// Queues a Reinstall of `parts` on one machine.
    pub(super) fn queue_kit_reinstall(&mut self, device_id: &str, parts: Vec<ComponentId>) -> bool {
        self.queue_kit_scope(device_id, Scope::reinstall(parts))
    }

    fn queue_kit_scope(&mut self, device_id: &str, scope: Scope) -> bool {
        if device_id != LOCAL_DEVICE_ID {
            self.queue_device_kit(device_id, KitJob::Apply(scope));
            return true;
        }
        let merged = merge_scopes(self.local_kit_pending.take(), scope);
        self.local_kit_pending = Some(merged);
        self.set_kit_busy(LOCAL_DEVICE_ID);
        true
    }

    /// Queues kit work for a device and starts its worker when its helper is
    /// connected. A device that is not connected yet is asked to connect,
    /// and the work runs when it does; its row is busy only while the work
    /// can actually run.
    pub(super) fn queue_device_kit(&mut self, device_id: &str, job: KitJob) {
        let merged = match self.device_kit_pending.remove(device_id) {
            Some(before) => before.merge(job),
            None => job,
        };
        let installs = matches!(merged, KitJob::Apply(_));
        self.device_kit_pending.insert(device_id.to_owned(), merged);
        let reachable = |runtime: &Runtime| {
            matches!(
                runtime.device_hosts.get(device_id).map(|host| &host.phase),
                Some(HostPhase::Ready { .. } | HostPhase::Connecting)
            )
        };
        if !reachable(self) {
            self.start_device_host(device_id);
        }
        if installs && reachable(self) {
            self.set_kit_busy(device_id);
        }
        self.start_device_kit(device_id);
    }

    /// Starts the device's kit worker when it has work, its helper is
    /// connected and no worker runs for it yet.
    pub(super) fn start_device_kit(&mut self, device_id: &str) {
        let removal = self.device_kit_removals.contains_key(device_id);
        let queued = self.device_kit_pending.contains_key(device_id)
            && matches!(
                self.device_hosts.get(device_id).map(|host| &host.phase),
                Some(HostPhase::Ready { .. })
            );
        if self.device_kit_running.contains(device_id) || !(removal || queued) {
            return;
        }
        if !self.spawn_device_kit_worker(device_id) {
            self.clear_kit_busy(device_id);
        }
    }

    /// One worker per device, so its kit calls run one at a time and a
    /// removal never runs beside an install on the same helper.
    fn spawn_device_kit_worker(&mut self, device_id: &str) -> bool {
        let Some(context) = self.worker_context.clone() else {
            return false;
        };
        self.device_kit_running.insert(device_id.to_owned());
        match crate::kit::spawn_device_worker(
            context.runtime,
            context.notifier,
            device_id.to_owned(),
        ) {
            Ok(()) => true,
            Err(error) => {
                self.device_kit_running.remove(device_id);
                crate::diagnostic!(serde_json::json!({
                    "component": "kit",
                    "kind": "worker.spawn_failed",
                    "device_id": device_id,
                    "error": error.to_string(),
                }));
                false
            }
        }
    }

    /// The device is being removed: with its helper connected, Hide's parts
    /// come off it on the device's kit worker, after any kit call already
    /// running there, and the helper connection closes after that. Without
    /// one, or while another registration still reaches the same account,
    /// what stays on the device is recorded (B23, B24). Answers why the kit
    /// stays when the removal was not started.
    pub(super) fn queue_device_kit_removal(
        &mut self,
        registration: &crate::model::DeviceRegistration,
    ) -> Result<(), &'static str> {
        let device_id = registration.id.as_str();
        self.device_kit_pending.remove(device_id);
        self.kit_states.remove(device_id);
        let left = |reason: &'static str| {
            crate::diagnostic!(serde_json::json!({
                "component": "kit",
                "kind": "device.kit_left",
                "device_id": device_id,
                "reason": reason,
                "left": LEFT_ON_DEVICE,
            }));
            Err(reason)
        };
        if self.registration_sharing_account(registration).is_some() {
            return left("another registered device reaches the same account on that machine");
        }
        let channel =
            self.device_hosts.get_mut(device_id).and_then(|host| {
                match std::mem::replace(&mut host.phase, HostPhase::NotAllowed) {
                    HostPhase::Ready { host: channel, .. } if channel.closed_reason().is_none() => {
                        Some(channel)
                    }
                    other => {
                        host.phase = other;
                        None
                    }
                }
            });
        let Some(channel) = channel else {
            return left("its helper was not connected");
        };
        self.device_kit_removals.insert(
            device_id.to_owned(),
            DeviceKitCall {
                work: DeviceKitWork::Remove,
                channel: Arc::clone(&channel),
                cli_dir: registration
                    .host_consent
                    .as_ref()
                    .and_then(|consent| consent.cli_dir.clone())
                    .unwrap_or_else(|| self.host_cli_dir.clone()),
                herdr_socket: registration.herdr_socket_path.clone(),
                retirement_projects: Vec::new(),
            },
        );
        self.device_kit_removing.insert(device_id.to_owned());
        if !self.device_kit_running.contains(device_id) && !self.spawn_device_kit_worker(device_id)
        {
            self.device_kit_removals.remove(device_id);
            self.device_kit_removing.remove(device_id);
            channel.close("device removed");
            return left("the kit worker could not start");
        }
        Ok(())
    }

    /// Whether Hide is still taking its kit off a removed device; a new
    /// connection to it waits, so the removal never deletes what the new
    /// connection installs.
    pub(super) fn device_kit_removing(&self, device_id: &str) -> bool {
        self.device_kit_removing.contains(device_id)
    }

    /// The device's next kit call, or `None` when its worker should stop:
    /// nothing is queued, or the helper is no longer connected (the work
    /// then waits for the next connection).
    pub(crate) fn take_device_kit_call(&mut self, device_id: &str) -> Option<DeviceKitCall> {
        if let Some(removal) = self.device_kit_removals.remove(device_id) {
            return Some(removal);
        }
        let channel = match self.device_hosts.get(device_id).map(|host| &host.phase) {
            Some(HostPhase::Ready { host, .. }) if host.closed_reason().is_none() => {
                Arc::clone(host)
            }
            _ => {
                self.device_kit_running.remove(device_id);
                self.clear_kit_busy(device_id);
                return None;
            }
        };
        let registration = self
            .snapshot
            .ui_state
            .device_registrations
            .iter()
            .find(|registration| registration.id == device_id)
            .cloned();
        let (Some(registration), Some(job)) =
            (registration, self.device_kit_pending.remove(device_id))
        else {
            self.device_kit_running.remove(device_id);
            return None;
        };
        let cli_dir = registration
            .host_consent
            .and_then(|consent| consent.cli_dir)
            .unwrap_or_else(|| self.host_cli_dir.clone());
        Some(DeviceKitCall {
            work: DeviceKitWork::Job(job),
            channel,
            cli_dir,
            herdr_socket: registration.herdr_socket_path,
            retirement_projects: self.retirement_projects(device_id),
        })
    }

    /// Stores what a device's kit call answered. A call that failed keeps
    /// what the device last reported; a device never read says why it could
    /// not be, until the next connection or Settings reads it again.
    pub(crate) fn ingest_device_kit_answer(
        &mut self,
        device_id: &str,
        answer: DeviceKitAnswer,
    ) -> bool {
        if let DeviceKitAnswer::Report(_) = &answer
            && (!self.device_registration_exists(device_id) || self.device_kit_removing(device_id))
        {
            // The device went while the call ran: its kit state went with it,
            // and a late answer must not bring it back for a device added
            // again under the same id.
            return false;
        }
        match answer {
            DeviceKitAnswer::Removed(removed) => self.ingest_device_kit_removal(device_id, removed),
            DeviceKitAnswer::Report(Ok(report)) => self.ingest_kit_report(device_id, &report),
            DeviceKitAnswer::Report(Err(reason)) => {
                // The call never ran to a report, so the choice it carried
                // was not delivered: the next report that waits sends it again.
                self.reopen_first_run_choice(device_id);
                crate::diagnostic!(serde_json::json!({
                    "component": "kit",
                    "kind": "device.call_failed",
                    "device_id": device_id,
                    "reason": reason,
                }));
                let mut state = self.kit_state(device_id);
                state.busy = self.kit_install_queued(device_id);
                if state.components.is_empty() {
                    state.unavailable = Some(format!(
                        "Hide could not read its kit on this device: {reason}"
                    ));
                }
                self.set_kit_state(device_id, state)
            }
        }
    }

    /// Records what a removal took off a device and what stayed (B23, B24).
    /// A device added again while it ran connects now.
    fn ingest_device_kit_removal(
        &mut self,
        device_id: &str,
        removed: Result<hide_host::protocol::KitRemoved, String>,
    ) -> bool {
        match removed {
            Ok(removed) => crate::diagnostic!(serde_json::json!({
                "component": "kit",
                "kind": "device.kit_removed",
                "device_id": device_id,
                "components": removed.kit.components.iter().map(|(id, outcome)| {
                    serde_json::json!({ "id": id.code(), "outcome": outcome })
                }).collect::<Vec<_>>(),
                // The agents' hooks and skill stubs, a failed one included.
                "agents": removed.kit.agents.iter().map(|(piece, outcome)| {
                    serde_json::json!({ "piece": piece, "outcome": outcome })
                }).collect::<Vec<_>>(),
                "helper_root": removed.helper_root,
            })),
            Err(reason) => crate::diagnostic!(serde_json::json!({
                "component": "kit",
                "kind": "device.kit_left",
                "device_id": device_id,
                "reason": reason,
                "left": LEFT_ON_DEVICE,
            })),
        }
        self.device_kit_removing.remove(device_id);
        self.device_registration_exists(device_id) && self.start_device_host(device_id)
    }

    /// Queued work cannot run now: the row stops saying it is working.
    pub(super) fn clear_kit_busy(&mut self, device_id: &str) {
        let mut state = self.kit_state(device_id);
        if state.busy {
            state.busy = false;
            self.set_kit_state(device_id, state);
        }
    }

    /// The device's consent was withdrawn or the device is going away:
    /// nothing queued for it runs.
    pub(super) fn forget_device_kit_work(&mut self, device_id: &str) {
        self.device_kit_pending.remove(device_id);
        self.reopen_first_run_choice(device_id);
        self.clear_kit_busy(device_id);
    }

    /// The hook parts of this Mac that a Reinstall would repair, for
    /// Memory's "update hooks".
    pub(super) fn local_hook_parts_to_repair(&self) -> Vec<ComponentId> {
        self.kit_states
            .get(LOCAL_DEVICE_ID)
            .map(|state| {
                state
                    .components
                    .iter()
                    .filter(|part| {
                        matches!(
                            part.id,
                            ComponentId::ClaudeCodeHook | ComponentId::CodexHook
                        ) && part.state.needs_attention()
                    })
                    .map(|part| part.id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Each agent's open sessions on one machine, counted by whether Hide
    /// hears them (PRD settings-cleanup B16, B17, B19).
    ///
    /// Only the sessions that exist now are counted: the walk is over the
    /// panes the snapshot holds, so a closed session leaves the count on the
    /// next pass and nothing accumulates per session (D-23). The work is one
    /// pass over the machine's panes each time the agent lineage pass or a
    /// device's session republishes, not a per-tick cost, and the list it
    /// builds is capped at [`crate::model::MAX_NOT_CONNECTED_SESSIONS`].
    fn agent_sessions(
        &self,
        device_id: &str,
    ) -> std::collections::BTreeMap<&'static str, crate::model::KitAgentSessionsSnapshot> {
        use crate::model::{
            KitAgentSessionsSnapshot, MAX_NOT_CONNECTED_SESSIONS, NotConnectedSessionSnapshot,
        };
        let remote = self
            .snapshot
            .status
            .remote
            .iter()
            .find(|status| status.target_id == device_id)
            .and_then(|status| status.session.as_ref());
        let (workspaces, agents) = if device_id == LOCAL_DEVICE_ID {
            (
                &self.snapshot.navigator.workspaces,
                &self.snapshot.navigator.agents,
            )
        } else if let Some(session) = remote {
            (&session.workspaces, &session.agents)
        } else {
            return std::collections::BTreeMap::new();
        };
        let mut sessions: std::collections::BTreeMap<&'static str, KitAgentSessionsSnapshot> =
            hide_kit::agents::ADAPTERS
                .iter()
                .filter(|adapter| adapter.supports(hide_kit::Feature::Letters))
                .map(|adapter| (adapter.id, KitAgentSessionsSnapshot::default()))
                .collect();
        let panes = workspaces
            .iter()
            .flat_map(|workspace| workspace.checkouts.iter())
            .flat_map(|checkout| checkout.tabs.iter())
            .flat_map(|tab| tab.panes.iter());
        for pane in panes {
            let local_pane = !crate::agent_hooks::is_remote_pane(&pane.id);
            if (device_id == LOCAL_DEVICE_ID) != local_pane {
                continue;
            }
            let Some(connection) = pane
                .children
                .as_ref()
                .and_then(|children| children.connection)
            else {
                continue;
            };
            let Some(agent) = agents.iter().find(|agent| agent.pane_id == pane.id) else {
                continue;
            };
            let Some(adapter_id) = crate::agent_hooks::runtime_of(&agent.agent_kind)
                .map(crate::agent_hooks::adapter_id)
            else {
                continue;
            };
            let Some(row) = sessions.get_mut(adapter_id) else {
                continue;
            };
            match connection.reason {
                None => row.connected += 1,
                Some(reason) if row.not_connected.len() < MAX_NOT_CONNECTED_SESSIONS => {
                    row.not_connected.push(NotConnectedSessionSnapshot {
                        pane_id: pane.id.clone(),
                        title: agent.identity_label.clone(),
                        project: agent.workspace_label.clone(),
                        reason,
                    });
                }
                Some(_) => row.not_connected_hidden += 1,
            }
        }
        sessions
    }

    /// Puts [`Self::agent_sessions`] on a kit snapshot's agent rows; an agent
    /// with no connection to judge keeps `None`.
    fn fill_agent_sessions(&self, device_id: &str, kit: &mut KitSnapshot) {
        let sessions = self.agent_sessions(device_id);
        for agent in &mut kit.agents {
            agent.sessions = sessions.get(agent.id.as_str()).cloned();
        }
    }

    /// Recounts every machine's sessions onto its kit rows after the panes
    /// changed; returns whether any count moved.
    pub(super) fn refresh_agent_sessions(&mut self) -> bool {
        let ids = self
            .snapshot
            .navigator
            .devices
            .iter()
            .map(|device| device.id.clone())
            .collect::<Vec<_>>();
        let mut changed = false;
        for id in ids {
            let sessions = self.agent_sessions(&id);
            let Some(device) = self
                .snapshot
                .navigator
                .devices
                .iter_mut()
                .find(|device| device.id == id)
            else {
                continue;
            };
            for agent in &mut device.kit.agents {
                let next = sessions.get(agent.id.as_str()).cloned();
                if agent.sessions != next {
                    agent.sessions = next;
                    changed = true;
                }
            }
        }
        changed
    }

    /// A machine's kit as its row shows it. A device the helper may not be
    /// installed on, or whose platform this build does not carry, installs
    /// nothing and says why (B17, B21); otherwise its last report stands.
    pub(super) fn kit_view(&self, device_id: &str) -> KitSnapshot {
        let mut view = self.kit_view_without_sessions(device_id);
        self.fill_agent_sessions(device_id, &mut view);
        view
    }

    fn kit_view_without_sessions(&self, device_id: &str) -> KitSnapshot {
        if device_id == LOCAL_DEVICE_ID {
            return self.kit_state(device_id);
        }
        let mut view = match self.device_kit_declined(device_id) {
            Some(declined) => KitSnapshot::unavailable(declined.reason()),
            None => self.kit_state(device_id),
        };
        view.shares_account_with = self
            .snapshot
            .ui_state
            .device_registrations
            .iter()
            .find(|registration| registration.id == device_id)
            .and_then(|registration| self.registration_sharing_account(registration))
            .map(|other| other.label.clone());
        view
    }

    /// Why Hide's kit does not run on a device now, if it does not.
    pub(super) fn device_kit_declined(&self, device_id: &str) -> Option<KitDeclined> {
        let host = self.host_snapshot(device_id);
        match (host.consent.as_str(), host.state.as_str()) {
            ("none", _) => Some(KitDeclined::NoConsent),
            ("outdated", _) => Some(KitDeclined::ConsentOutdated),
            (_, "unsupported") => Some(KitDeclined::Unsupported(host.message.unwrap_or_else(
                || "This Hide build does not support this device's platform".to_owned(),
            ))),
            _ => None,
        }
    }

    /// Another registration that reaches the same account on the same
    /// machine: its consent bound the same user and host key, as two
    /// registrations for two Herdr servers there do. The hooks, the `hide`
    /// link and the helper root belong to that account, not to one
    /// registration, so removing one of them leaves those for the other.
    fn registration_sharing_account(
        &self,
        registration: &crate::model::DeviceRegistration,
    ) -> Option<&crate::model::DeviceRegistration> {
        fn account(registration: &crate::model::DeviceRegistration) -> Option<(&str, &str)> {
            let identity = registration.host_consent.as_ref()?.identity.as_ref()?;
            Some((&identity.user, &identity.host_key_sha256))
        }
        let own = account(registration)?;
        self.snapshot
            .ui_state
            .device_registrations
            .iter()
            .find(|other| other.id != registration.id && account(other) == Some(own))
    }

    /// What a Codex start on this machine passes about the shared daemon,
    /// from its kit's last report (PRD overview-request-view D-20).
    pub(super) fn codex_daemon(&self, device_id: &str) -> crate::codex_launch::CodexDaemon {
        self.kit_states
            .get(device_id)
            .map(crate::codex_launch::CodexDaemon::from_kit)
            .unwrap_or_default()
    }

    /// [`Self::codex_daemon`] for the machine a pane id belongs to.
    pub(super) fn codex_daemon_for_pane(&self, pane_id: &str) -> crate::codex_launch::CodexDaemon {
        let device = pane_id
            .strip_prefix("remote:")
            .and_then(|rest| rest.split_once(":pane:"))
            .map_or(LOCAL_DEVICE_ID, |(device, _)| device);
        self.codex_daemon(device)
    }

    pub(super) fn kit_state(&self, device_id: &str) -> KitSnapshot {
        self.kit_states.get(device_id).cloned().unwrap_or_default()
    }

    fn set_kit_busy(&mut self, device_id: &str) {
        let mut state = self.kit_state(device_id);
        state.busy = true;
        self.set_kit_state(device_id, state);
    }

    pub(super) fn set_kit_state(&mut self, device_id: &str, snapshot: KitSnapshot) -> bool {
        if self.kit_states.get(device_id) == Some(&snapshot) {
            return false;
        }
        self.kit_states.insert(device_id.to_owned(), snapshot);
        self.refresh_device_snapshots();
        // A device's agent panes are judged against its kit.
        if device_id != LOCAL_DEVICE_ID {
            self.refresh_device_catalog(device_id);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_for_one_machine_merge_into_the_widest() {
        let reinstall =
            |parts: &[ComponentId]| KitJob::Apply(Scope::reinstall(parts.iter().copied()));
        assert_eq!(
            KitJob::Status.merge(KitJob::Apply(Scope::automatic())),
            KitJob::Apply(Scope::automatic())
        );
        assert_eq!(
            KitJob::Apply(Scope::automatic()).merge(KitJob::Status),
            KitJob::Apply(Scope::automatic())
        );
        assert_eq!(
            reinstall(&[ComponentId::CoordinationRetirement])
                .merge(KitJob::Apply(Scope::automatic())),
            reinstall(&[ComponentId::CoordinationRetirement])
        );
        assert_eq!(
            reinstall(&[ComponentId::CoordinationRetirement]).merge(reinstall(&[
                ComponentId::Cli,
                ComponentId::CoordinationRetirement
            ])),
            reinstall(&[ComponentId::Cli, ComponentId::CoordinationRetirement])
        );
        assert_eq!(KitJob::Status.merge(KitJob::Status), KitJob::Status);
        // The operator's later switch wins for the agent it names.
        assert_eq!(
            KitJob::Apply(Scope::agents([], ["pi"]))
                .merge(KitJob::Apply(Scope::agents(["pi"], []))),
            KitJob::Apply(Scope::agents(["pi"], []))
        );
    }
}
