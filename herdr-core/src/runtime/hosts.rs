//! Each registered device's file host: the operator's consent, the helper
//! connection it allows, and the one state every file and Git action on that
//! device reads (PRD S5.5 D-20, D-23, B50-B52).
//!
//! A device without consent never gets a helper connection. Consent given at
//! registration or later is bound to the SSH identity the helper first runs
//! on; a device that later answers with another account, address or host key
//! is refused until the operator allows it again. Revoking stops the
//! connection admitting work at once, whoever still holds it, and refuses a
//! request still waiting for a slot; work already admitted answers on it and
//! settles to its real result before it closes, a save held for the helper is
//! dropped unsent, and nothing on the device is removed.

use std::sync::Arc;

use super::*;
use crate::host_access::HostChannel;
use crate::model::{DeviceHostSnapshot, HostConsent};
use crate::remote::host::{
    self, EstablishError, Established, HOST_CONSENT_CONTRACT, HelperPackages,
};

pub(super) enum HostPhase {
    NotAllowed,
    Connecting,
    Ready {
        host: Arc<dyn HostChannel>,
        platform: String,
        helper_path: String,
    },
    IdentityChanged(String),
    Unsupported(String),
    Unavailable(String),
}

pub(super) struct DeviceHost {
    pub(super) phase: HostPhase,
    /// Taken from the runtime-wide counter on every connection attempt and
    /// close, so a late answer from an older attempt, including one made
    /// before the device was removed and added again, is recognised and
    /// dropped.
    pub(super) generation: u64,
}

impl Runtime {
    pub(super) fn host_helper_root(&self) -> String {
        self.host_helper_root.clone()
    }

    pub(super) fn device_registration_exists(&self, device_id: &str) -> bool {
        self.device_registration(device_id).is_some()
    }

    fn device_registration(&self, device_id: &str) -> Option<&crate::model::DeviceRegistration> {
        self.snapshot
            .ui_state
            .device_registrations
            .iter()
            .find(|registration| registration.id == device_id)
    }

    /// Whether a consent still covers what this build would do: the same
    /// contract and the same install root.
    fn consent_current(&self, consent: &HostConsent) -> bool {
        consent.contract == HOST_CONSENT_CONTRACT && consent.helper_root == self.host_helper_root
    }

    /// A fresh consent in this build's scope, unbound until the first
    /// connection.
    pub(super) fn new_host_consent(&self) -> HostConsent {
        HostConsent {
            contract: HOST_CONSENT_CONTRACT,
            helper_root: self.host_helper_root(),
            granted_at_unix_ms: now_unix_ms(),
            identity: None,
        }
    }

    /// Gives or withdraws consent for one device.
    pub(super) fn set_host_consent(&mut self, device_id: &str, allow: bool) -> bool {
        let fresh = self.new_host_consent();
        let Some(registration) = self
            .snapshot
            .ui_state
            .device_registrations
            .iter_mut()
            .find(|registration| registration.id == device_id)
        else {
            self.set_error(
                "device.host.unknown_device",
                format!("Device {device_id} is not registered"),
                false,
            );
            return true;
        };
        if allow {
            registration.host_consent = Some(fresh);
            self.persist_current_ui_state();
            crate::diagnostic!(serde_json::json!({
                "component": "remote_host",
                "kind": "host.consent_granted",
                "target": device_id,
                "contract": HOST_CONSENT_CONTRACT,
            }));
            self.close_device_host(device_id, "consent renewed");
            self.start_device_host(device_id);
            self.relist_remote_files(device_id);
        } else {
            registration.host_consent = None;
            self.persist_current_ui_state();
            crate::diagnostic!(serde_json::json!({
                "component": "remote_host",
                "kind": "host.consent_revoked",
                "target": device_id,
            }));
            // New work stops here: the channel is no longer handed out and
            // admits nothing more from a worker still holding it. Work
            // already admitted answers on the old connection, which closes
            // once it is idle, so a save in flight settles to its real
            // result rather than becoming unknown (B52).
            if self.device_hosts.contains_key(device_id) {
                self.advance_host_generation(device_id);
                let entry = self.device_host_entry(device_id);
                if let HostPhase::Ready { host, .. } =
                    std::mem::replace(&mut entry.phase, HostPhase::NotAllowed)
                {
                    host.close_when_idle("consent revoked");
                }
            }
            self.set_host_phase(device_id, HostPhase::NotAllowed);
            // A save held for the helper to become ready would otherwise go
            // out on its own if consent is given again later (B52).
            self.release_held_saves(
                device_id,
                "Hide's helper is no longer allowed on this device",
            );
        }
        self.refresh_device_snapshots();
        true
    }

    /// Starts a helper connection for a consented device unless one is
    /// running or starting. Returns whether the device row changed.
    pub(super) fn start_device_host(&mut self, device_id: &str) -> bool {
        let Some(registration) = self.device_registration(device_id).cloned() else {
            return false;
        };
        if matches!(
            self.device_hosts.get(device_id).map(|host| &host.phase),
            Some(HostPhase::Connecting | HostPhase::Ready { .. })
        ) {
            return false;
        }
        let generation = self.advance_host_generation(device_id);
        let Some(consent) = registration.host_consent.clone() else {
            self.set_host_phase(device_id, HostPhase::NotAllowed);
            return self.refresh_device_snapshots();
        };
        if !self.consent_current(&consent) {
            self.set_host_phase(device_id, HostPhase::NotAllowed);
            return self.refresh_device_snapshots();
        }
        let Some(client) = self
            .remote_connections
            .get(device_id)
            .map(|connection| Arc::clone(&connection.client))
        else {
            self.set_host_phase(
                device_id,
                HostPhase::Unavailable("The device has no SSH connection yet".to_owned()),
            );
            return self.refresh_device_snapshots();
        };
        let Some(context) = self.worker_context.clone() else {
            self.set_host_phase(
                device_id,
                HostPhase::Unavailable("The helper worker is unavailable".to_owned()),
            );
            return self.refresh_device_snapshots();
        };
        self.set_host_phase(device_id, HostPhase::Connecting);
        let packages = self.host_packages.clone();
        let device = device_id.to_owned();
        let spawned = thread::Builder::new()
            .name("herdr-core-device-host".to_owned())
            .spawn(move || {
                let close_context = context.clone();
                let close_device = device.clone();
                let on_close = Box::new(move |reason: String| {
                    let Some(runtime) = close_context.runtime.upgrade() else {
                        return;
                    };
                    let changed = match runtime.lock() {
                        Ok(mut guard) => {
                            guard.ingest_host_closed(&close_device, generation, reason)
                        }
                        Err(_) => return,
                    };
                    drop(runtime);
                    if changed {
                        close_context.notifier.notify();
                    }
                });
                let result = host::establish(&client, &packages, &consent, on_close);
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_host_established(&device, generation, result),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawned {
            self.set_host_phase(
                device_id,
                HostPhase::Unavailable(format!("The helper worker could not start: {error}")),
            );
        }
        self.refresh_device_snapshots()
    }

    fn device_host_entry(&mut self, device_id: &str) -> &mut DeviceHost {
        self.device_hosts
            .entry(device_id.to_owned())
            .or_insert(DeviceHost {
                phase: HostPhase::NotAllowed,
                generation: 0,
            })
    }

    fn set_host_phase(&mut self, device_id: &str, phase: HostPhase) {
        self.device_host_entry(device_id).phase = phase;
    }

    pub(super) fn advance_host_generation(&mut self, device_id: &str) -> u64 {
        self.last_host_generation += 1;
        let generation = self.last_host_generation;
        self.device_host_entry(device_id).generation = generation;
        generation
    }

    pub(super) fn ingest_host_established(
        &mut self,
        device_id: &str,
        generation: u64,
        result: Result<Established, EstablishError>,
    ) -> bool {
        let current = self
            .device_hosts
            .get(device_id)
            .is_some_and(|host| host.generation == generation);
        if !current {
            if let Ok(established) = result {
                established.host.close("superseded by a newer attempt");
            }
            return false;
        }
        match result {
            Ok(established) => {
                let mut bound_now = false;
                if let Some(consent) = self
                    .snapshot
                    .ui_state
                    .device_registrations
                    .iter_mut()
                    .find(|registration| registration.id == device_id)
                    .and_then(|registration| registration.host_consent.as_mut())
                    && consent.identity.is_none()
                {
                    consent.identity = Some(established.identity.clone());
                    bound_now = true;
                }
                if bound_now {
                    self.persist_current_ui_state();
                }
                crate::diagnostic!(serde_json::json!({
                    "component": "remote_host",
                    "kind": "host.ready",
                    "target": device_id,
                    "generation": generation,
                    "installed": established.installed,
                    "platform": format!("{} {}", established.hello.os, established.hello.arch),
                    "consent_bound": bound_now,
                }));
                self.set_host_phase(
                    device_id,
                    HostPhase::Ready {
                        host: Arc::new(established.host),
                        platform: format!("{} {}", established.hello.os, established.hello.arch),
                        helper_path: established.helper_path,
                    },
                );
                self.settle_device_saves(device_id);
                self.reset_device_facts(device_id);
                self.relist_remote_files(device_id);
            }
            Err(error) => {
                let message = error.to_string();
                crate::diagnostic!(serde_json::json!({
                    "component": "remote_host",
                    "kind": "host.failed",
                    "target": device_id,
                    "generation": generation,
                    "message": message,
                }));
                self.release_held_saves(device_id, &message);
                self.device_facts
                    .entry(device_id.to_owned())
                    .or_default()
                    .unavailable = Some(message.clone());
                let phase = match error {
                    EstablishError::IdentityChanged { .. } => HostPhase::IdentityChanged(message),
                    EstablishError::Unsupported(_) => HostPhase::Unsupported(message),
                    _ => HostPhase::Unavailable(message),
                };
                self.set_host_phase(device_id, phase);
                self.refresh_device_catalog(device_id);
            }
        }
        self.refresh_device_snapshots();
        true
    }

    pub(super) fn device_host_generation(&self, device_id: &str) -> u64 {
        self.device_hosts
            .get(device_id)
            .map_or(0, |host| host.generation)
    }

    pub(super) fn device_host_connecting(&self, device_id: &str) -> bool {
        matches!(
            self.device_hosts.get(device_id).map(|host| &host.phase),
            Some(HostPhase::Connecting)
        )
    }

    pub(super) fn ingest_host_closed(
        &mut self,
        device_id: &str,
        generation: u64,
        reason: String,
    ) -> bool {
        let Some(host) = self.device_hosts.get_mut(device_id) else {
            return false;
        };
        if host.generation != generation || !matches!(host.phase, HostPhase::Ready { .. }) {
            return false;
        }
        host.phase = HostPhase::Unavailable(format!("The device helper disconnected: {reason}"));
        self.refresh_device_snapshots();
        true
    }

    /// Ends the helper connection, if any. Work still waiting settles as
    /// unknown in its own caller.
    pub(super) fn close_device_host(&mut self, device_id: &str, reason: &str) {
        if self.device_hosts.contains_key(device_id) {
            self.advance_host_generation(device_id);
            let host = self.device_host_entry(device_id);
            if let HostPhase::Ready { host: remote, .. } =
                std::mem::replace(&mut host.phase, HostPhase::Unavailable(reason.to_owned()))
            {
                remote.close(reason);
            }
        }
    }

    pub(super) fn forget_device_host(&mut self, device_id: &str) {
        self.close_device_host(device_id, "device removed");
        self.device_hosts.remove(device_id);
        self.forget_device_catalog(device_id);
    }

    /// Where a device's file or Git work runs, or the sentence that says
    /// why it cannot run now. This machine answers in process; an SSH device
    /// answers through its helper, and an unavailable helper on a consented
    /// device is asked for again here, so the next action finds it ready.
    pub(crate) fn device_channel(
        &mut self,
        device_id: &str,
    ) -> Result<Arc<dyn HostChannel>, String> {
        if device_id == crate::workspace::LOCAL_DEVICE_ID {
            return Ok(Arc::clone(&self.local_host));
        }
        match self.device_hosts.get(device_id).map(|host| &host.phase) {
            Some(HostPhase::Ready { host, .. }) if host.closed_reason().is_none() => {
                return Ok(Arc::clone(host));
            }
            Some(HostPhase::Ready { .. }) | Some(HostPhase::Unavailable(_)) | None => {
                self.start_device_host(device_id);
            }
            _ => {}
        }
        Err(self
            .host_snapshot(device_id)
            .message
            .unwrap_or_else(|| "The device helper is not ready".to_owned()))
    }

    /// The host row for one device, read by Settings and by every file or
    /// Git surface that needs to say why it cannot act.
    pub(super) fn host_snapshot(&self, device_id: &str) -> DeviceHostSnapshot {
        let consent = self
            .device_registration(device_id)
            .and_then(|registration| registration.host_consent.as_ref());
        let current = consent.is_some_and(|consent| self.consent_current(consent));
        let mut snapshot = DeviceHostSnapshot {
            consent: match consent {
                None => "none",
                Some(_) if current => "granted",
                Some(_) => "outdated",
            }
            .to_owned(),
            helper_root: Some(
                consent
                    .filter(|_| current)
                    .map(|consent| consent.helper_root.clone())
                    .unwrap_or_else(|| self.host_helper_root()),
            ),
            contract: HOST_CONSENT_CONTRACT,
            bound_identity: consent
                .and_then(|consent| consent.identity.as_ref())
                .map(|identity| identity.describe()),
            granted_at_unix_ms: consent.map(|consent| consent.granted_at_unix_ms),
            state: "not_allowed".to_owned(),
            message: None,
            platform: None,
            helper_path: None,
        };
        let phase = self.device_hosts.get(device_id).map(|host| &host.phase);
        let (state, message) = match (consent, current, phase) {
            (None, _, _) => (
                "not_allowed",
                Some("Allow Hide's helper on this device in Settings to use its files and Git".to_owned()),
            ),
            (Some(_), false, _) => (
                "not_allowed",
                Some("This version of Hide needs a wider permission on this device; review and allow it again in Settings".to_owned()),
            ),
            (Some(_), true, Some(HostPhase::Ready { host, platform, helper_path })) => {
                snapshot.platform = Some(platform.clone());
                snapshot.helper_path = Some(helper_path.clone());
                match host.closed_reason() {
                    None => ("ready", None),
                    Some(reason) => ("unavailable", Some(format!("The device helper disconnected: {reason}"))),
                }
            }
            (Some(_), true, Some(HostPhase::Connecting)) => ("connecting", Some("Connecting to the device helper…".to_owned())),
            (Some(_), true, Some(HostPhase::IdentityChanged(message))) => ("identity_changed", Some(message.clone())),
            (Some(_), true, Some(HostPhase::Unsupported(message))) => ("unsupported", Some(message.clone())),
            (Some(_), true, Some(HostPhase::Unavailable(message))) => ("unavailable", Some(message.clone())),
            (Some(_), true, Some(HostPhase::NotAllowed) | None) => (
                "unavailable",
                Some("The device helper has not connected yet".to_owned()),
            ),
        };
        snapshot.state = state.to_owned();
        snapshot.message = message;
        snapshot
    }

    /// Installs the helper packages this daemon carries; the Swift shell
    /// passes none.
    pub(super) fn helper_packages_from(options: &CoreOptions) -> (HelperPackages, String) {
        (
            HelperPackages::new(options.host_helper_dir.as_ref().map(PathBuf::from)),
            options
                .host_helper_root
                .clone()
                .unwrap_or_else(|| host::DEFAULT_HELPER_ROOT.to_owned()),
        )
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}
