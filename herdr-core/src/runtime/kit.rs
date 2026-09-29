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
    match (before, after) {
        (Some(Scope::Reinstall(mut before)), Scope::Reinstall(after)) => {
            before.extend(after);
            before.sort();
            before.dedup();
            Scope::Reinstall(before)
        }
        (Some(Scope::Reinstall(parts)), Scope::Automatic)
        | (Some(Scope::Automatic) | None, Scope::Reinstall(parts)) => Scope::Reinstall(parts),
        (Some(Scope::Automatic) | None, Scope::Automatic) => Scope::Automatic,
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
}

/// What a device's kit call answered.
pub(crate) enum DeviceKitAnswer {
    Report(Result<KitReport, String>),
    Removed(Result<hide_host::protocol::KitRemoved, String>),
}

/// What a removal leaves on a device whose helper is not connected (B24).
const LEFT_ON_DEVICE: [&str; 4] = [
    "Hide's hook entries",
    "the labels plugin link",
    "Hide's hide link",
    "the helper root",
];

impl Runtime {
    /// This Mac cannot run the kit at all, and its row says why (B11).
    pub(crate) fn set_local_kit_unavailable(&mut self, reason: &str) {
        self.set_kit_state(LOCAL_DEVICE_ID, KitSnapshot::unavailable(reason));
    }

    /// The worker exists: the launch pass is its first job (B1, B10).
    pub(crate) fn queue_local_kit_launch(&mut self) {
        self.local_kit_pending = Some(Scope::Automatic);
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
        let mut snapshot = KitSnapshot::from_report(report);
        snapshot.busy = self.kit_install_queued(device_id);
        self.set_kit_state(device_id, snapshot)
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
        let parts = state
            .components
            .iter()
            .filter(|part| part.state.needs_attention())
            .filter(|part| only.is_none_or(|only| only.contains(&part.id)))
            .map(|part| part.id)
            .collect::<Vec<_>>();
        // A second press after the first one repaired everything is the same
        // intent, already met (engineering rule 11).
        if parts.is_empty() {
            return false;
        }
        self.queue_kit_reinstall(device_id, parts)
    }

    /// Queues a Reinstall of `parts` on one machine.
    pub(super) fn queue_kit_reinstall(&mut self, device_id: &str, parts: Vec<ComponentId>) -> bool {
        if device_id != LOCAL_DEVICE_ID {
            self.queue_device_kit(device_id, KitJob::Apply(Scope::Reinstall(parts)));
            return true;
        }
        let merged = merge_scopes(self.local_kit_pending.take(), Scope::Reinstall(parts));
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
    /// one, what stays on the device is recorded (B23, B24). Answers whether
    /// the removal was started.
    pub(super) fn queue_device_kit_removal(
        &mut self,
        registration: &crate::model::DeviceRegistration,
    ) -> bool {
        let device_id = registration.id.as_str();
        self.device_kit_pending.remove(device_id);
        self.kit_states.remove(device_id);
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
        let left = |reason: &str| {
            crate::diagnostic!(serde_json::json!({
                "component": "kit",
                "kind": "device.kit_left",
                "device_id": device_id,
                "reason": reason,
                "left": LEFT_ON_DEVICE,
            }));
        };
        let Some(channel) = channel else {
            left("the device's helper was not connected");
            return false;
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
            },
        );
        self.device_kit_removing.insert(device_id.to_owned());
        if !self.device_kit_running.contains(device_id) && !self.spawn_device_kit_worker(device_id)
        {
            self.device_kit_removals.remove(device_id);
            self.device_kit_removing.remove(device_id);
            channel.close("device removed");
            left("the kit worker could not start");
            return false;
        }
        true
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
        match answer {
            DeviceKitAnswer::Removed(removed) => self.ingest_device_kit_removal(device_id, removed),
            DeviceKitAnswer::Report(Ok(report)) => self.ingest_kit_report(device_id, &report),
            DeviceKitAnswer::Report(Err(reason)) => {
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

    /// A machine's kit as its row shows it. A device the helper may not be
    /// installed on, or whose platform this build does not carry, installs
    /// nothing and says why (B17, B21); otherwise its last report stands.
    pub(super) fn kit_view(&self, device_id: &str) -> KitSnapshot {
        if device_id == LOCAL_DEVICE_ID {
            return self.kit_state(device_id);
        }
        let host = self.host_snapshot(device_id);
        match (host.consent.as_str(), host.state.as_str()) {
            ("none", _) => KitSnapshot::unavailable(
                "Hide installs its kit here once you allow its helper on this device",
            ),
            ("outdated", _) => KitSnapshot::unavailable(
                "Hide needs your permission again before it installs its kit here",
            ),
            (_, "unsupported") => KitSnapshot::unavailable(
                host.message
                    .as_deref()
                    .unwrap_or("This Hide build does not support this device's platform"),
            ),
            _ => self.kit_state(device_id),
        }
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
        let reinstall = |parts: &[ComponentId]| KitJob::Apply(Scope::Reinstall(parts.to_vec()));
        assert_eq!(
            KitJob::Status.merge(KitJob::Apply(Scope::Automatic)),
            KitJob::Apply(Scope::Automatic)
        );
        assert_eq!(
            KitJob::Apply(Scope::Automatic).merge(KitJob::Status),
            KitJob::Apply(Scope::Automatic)
        );
        assert_eq!(
            reinstall(&[ComponentId::Labels]).merge(KitJob::Apply(Scope::Automatic)),
            reinstall(&[ComponentId::Labels])
        );
        assert_eq!(
            reinstall(&[ComponentId::Labels])
                .merge(reinstall(&[ComponentId::Cli, ComponentId::Labels])),
            reinstall(&[ComponentId::Cli, ComponentId::Labels])
        );
        assert_eq!(KitJob::Status.merge(KitJob::Status), KitJob::Status);
    }
}
