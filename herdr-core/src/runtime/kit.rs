//! Each machine's install kit as the runtime holds it (PRD device-parity):
//! what the last check of each machine found, the install this Mac's kit
//! worker (`crate::kit`) runs next, and the operator's Reinstall.
//!
//! Nothing here touches a file or a process. The worker takes a job under the
//! lock, runs it with the lock released, and hands the report back.

use std::time::{Duration, Instant};

use hide_kit::{ComponentId, KitReport, Scope};

use super::Runtime;
use crate::model::KitSnapshot;
use crate::workspace::LOCAL_DEVICE_ID;

/// How often this Mac's kit is read again while Settings is on screen, so a
/// part the operator removed by hand shows up without a relaunch.
const LOCAL_STATUS_INTERVAL: Duration = Duration::from_secs(5);

/// What this Mac's kit worker is asked to do next.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum LocalKitJob {
    Apply(Scope),
    Status,
}

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
    pub(crate) fn take_local_kit_job(&mut self, now: Instant) -> Option<LocalKitJob> {
        if let Some(scope) = self.local_kit_pending.take() {
            return Some(LocalKitJob::Apply(scope));
        }
        if std::mem::take(&mut self.local_kit_check_requested)
            || self.settings_observed() && self.local_kit_next_status.is_none_or(|next| now >= next)
        {
            self.local_kit_next_status = Some(now + LOCAL_STATUS_INTERVAL);
            return Some(LocalKitJob::Status);
        }
        None
    }

    /// A Settings tab showing the kit opened: this Mac is read once. A
    /// daemon that cannot run the kit has nothing to read.
    pub(super) fn request_local_kit_check(&mut self) -> bool {
        if self
            .kit_states
            .get(LOCAL_DEVICE_ID)
            .is_some_and(|state| state.unavailable.is_some())
        {
            return false;
        }
        self.local_kit_check_requested = true;
        false
    }

    /// Stores what a check or an install found on one machine. A report that
    /// lands while another install is queued keeps the row busy.
    pub(crate) fn ingest_kit_report(&mut self, device_id: &str, report: &KitReport) -> bool {
        let mut snapshot = KitSnapshot::from_report(report);
        snapshot.busy = device_id == LOCAL_DEVICE_ID && self.local_kit_pending.is_some();
        self.set_kit_state(device_id, snapshot)
    }

    /// The operator pressed Reinstall on a machine's row: every part that is
    /// outdated, missing, removed or failed is installed again, and the parts
    /// in place are not touched (B8, B9).
    pub(super) fn request_kit_reinstall(
        &mut self,
        device_id: &str,
        only: Option<&[ComponentId]>,
    ) -> bool {
        let Some(state) = self.kit_states.get(device_id) else {
            self.set_error(
                "kit.unknown_machine",
                format!("Hide has not checked {device_id} yet, so there is nothing to reinstall"),
                false,
            );
            return true;
        };
        if let Some(reason) = state.unavailable.clone() {
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
            return self.queue_device_kit_reinstall(device_id, parts);
        }
        let merged = match self.local_kit_pending.take() {
            Some(Scope::Reinstall(mut before)) => {
                before.extend(parts);
                before.sort();
                before.dedup();
                before
            }
            // A Reinstall does everything the launch pass does and more.
            Some(Scope::Automatic) | None => parts,
        };
        self.local_kit_pending = Some(Scope::Reinstall(merged));
        self.set_kit_busy(LOCAL_DEVICE_ID);
        true
    }

    /// Queues a Reinstall on a device; its helper connection runs it
    /// (`remote/host.rs`), and a device that is not connected runs it when it
    /// connects, since the connection pass installs what is missing anyway.
    fn queue_device_kit_reinstall(&mut self, device_id: &str, parts: Vec<ComponentId>) -> bool {
        let merged = match self.device_kit_pending.remove(device_id) {
            Some(Scope::Reinstall(mut before)) => {
                before.extend(parts);
                before.sort();
                before.dedup();
                before
            }
            Some(Scope::Automatic) | None => parts,
        };
        self.device_kit_pending
            .insert(device_id.to_owned(), Scope::Reinstall(merged));
        self.set_kit_busy(device_id);
        true
    }

    /// Hands a device's queued Reinstall to the connection that runs it.
    pub(crate) fn take_device_kit_request(&mut self, device_id: &str) -> Option<Scope> {
        self.device_kit_pending.remove(device_id)
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
        true
    }
}
