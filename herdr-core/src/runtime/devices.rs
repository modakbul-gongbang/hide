//! The registered SSH devices: each one's connection is opened when it is
//! registered or when the core starts, torn down when it is removed, and
//! probed stage by stage when the operator asks for a test.
//!
//! Settings holds only a label and an SSH alias for a device. Everything else
//! is asked of the host: the alias resolves through `~/.ssh/config`, and the
//! Herdr socket is whatever `herdr status server --json` reports there, so no
//! path of another machine is ever written down here.

use std::sync::Arc;

use super::*;
use crate::model::{
    DeviceRegistration, DeviceTestSnapshot, DeviceTestStageSnapshot, RemoteFileListSnapshot,
    RemoteStatusSnapshot,
};
use crate::remote::{CapabilityReport, CapabilityState, RusshRemoteClient, SshAlias};

pub(super) struct RemoteDeviceConnection {
    pub(super) client: Arc<RusshRemoteClient>,
    /// `None` when the coordinator could not be started; the status entry
    /// then carries why.
    sync: Option<session_sync::SessionSyncHandle>,
    /// True while a connection test runs; one test per device at a time, so
    /// a second click cannot stack SSH handshakes behind the first.
    test_in_flight: bool,
}

impl Runtime {
    /// Connects every device the persisted registrations name. Called once,
    /// after the worker context exists, because a coordinator reports back
    /// through it.
    pub fn connect_registered_devices(&mut self) -> bool {
        let registrations = self.snapshot.ui_state.device_registrations.clone();
        let mut changed = false;
        for registration in &registrations {
            changed |= self.connect_remote_device(registration);
        }
        changed
    }

    /// Opens the remote status entry for a device and starts its coordinator.
    /// A device that cannot be started still gets its entry, carrying the
    /// reason, so the row never reads as "not attempted".
    pub(super) fn connect_remote_device(&mut self, registration: &DeviceRegistration) -> bool {
        let device_id = registration.id.clone();
        let Some(ssh_alias) = registration.ssh_alias.clone() else {
            return false;
        };
        if self.remote_connections.contains_key(&device_id) {
            return false;
        }
        if !self
            .snapshot
            .status
            .remote
            .iter()
            .any(|status| status.target_id == device_id)
        {
            self.snapshot.status.remote.push(RemoteStatusSnapshot {
                target_id: device_id.clone(),
                state: "not_connected".to_owned(),
                message: Some("Waiting for the first remote connection attempt".to_owned()),
                herdr_version: None,
                session: None,
                files: RemoteFileListSnapshot::idle(),
            });
        }
        if !self.remote_enabled {
            return self.ingest_remote_session(
                &device_id,
                Err(live::SessionFetchError::Unreachable(
                    "Remote features are disabled because the SSH agent socket is unavailable"
                        .to_owned(),
                )),
            );
        }
        let started = (|| {
            let home_path = self.home_path.as_ref().ok_or_else(|| {
                "HOME is unavailable, so the SSH config cannot be resolved".to_owned()
            })?;
            // The row is what the operator reads, so it names the fix; the
            // full staged diagnostic goes to the log below.
            let alias = SshAlias::from_config_file(&home_path.join(".ssh/config"), &ssh_alias)
                .map_err(|error| {
                    format!(
                        "SSH alias {ssh_alias} could not be read from ~/.ssh/config: {}",
                        error.diagnostic().reason
                    )
                })?;
            let client =
                Arc::new(RusshRemoteClient::new(alias).map_err(|error| error.to_string())?);
            let connector: Arc<dyn hide_herdr_client::ApiConnector> =
                Arc::new(client.herdr_api_connector());
            Ok::<_, String>((client, connector))
        })();
        let (client, connector) = match started {
            Ok(parts) => parts,
            Err(message) => {
                crate::diagnostic!(serde_json::json!({
                    "component": "remote_session_sync",
                    "kind": "device.connect_failed",
                    "target": device_id,
                    "message": message,
                }));
                return self.ingest_remote_session(
                    &device_id,
                    Err(live::SessionFetchError::Unreachable(message)),
                );
            }
        };
        let Some(context) = self.worker_context.clone() else {
            return self.ingest_remote_session(
                &device_id,
                Err(live::SessionFetchError::Unreachable(
                    "The remote session worker is unavailable".to_owned(),
                )),
            );
        };
        self.install_remote_control(live::RemoteControlContext::new(
            device_id.clone(),
            Arc::clone(&connector),
            context.runtime.clone(),
            context.notifier.clone(),
        ));
        self.install_remote_terminal(live::RemoteTerminalContext::new(
            device_id.clone(),
            Arc::clone(&client),
            context.runtime.clone(),
            context.notifier.clone(),
        ));
        self.install_remote_file_transport(
            device_id.clone(),
            RusshSftpTransport::new(Arc::clone(&client)),
        );
        let sync_context = session_sync::SessionSyncContext::remote(
            device_id.clone(),
            registration.label.clone(),
            connector,
            context.runtime.clone(),
            context.notifier.clone(),
        );
        let (sync, changed) = match session_sync::spawn(sync_context, None) {
            Ok(handle) => (Some(handle), false),
            Err(message) => {
                crate::diagnostic!(serde_json::json!({
                    "component": "remote_session_sync",
                    "kind": "coordinator.spawn_failed",
                    "target": device_id,
                    "message": message,
                }));
                let changed = self.ingest_remote_session(
                    &device_id,
                    Err(live::SessionFetchError::Unreachable(message)),
                );
                (None, changed)
            }
        };
        self.remote_connections.insert(
            device_id,
            RemoteDeviceConnection {
                client,
                sync,
                test_in_flight: false,
            },
        );
        changed || self.refresh_device_snapshots()
    }

    /// Forgets everything the core holds for a device: its coordinator,
    /// transports, status entry, pending operations and projected panes. The
    /// coordinator is joined later, off the lock, by whoever drains
    /// `take_retired_remote_syncs`.
    pub(super) fn disconnect_remote_device(&mut self, device_id: &str) {
        if let Some(connection) = self.remote_connections.remove(device_id)
            && let Some(sync) = connection.sync
        {
            self.retired_remote_syncs.push(sync);
        }
        self.remote_controls.remove(device_id);
        self.remote_terminals.remove(device_id);
        self.remote_file_transports.remove(device_id);
        self.remote_connection_generations.remove(device_id);
        self.remote_operations
            .retain(|(target_id, _), _| target_id != device_id);
        self.remote_tab_creations_in_flight
            .retain(|(target_id, _, _, _)| target_id != device_id);
        self.remote_device_tests.remove(device_id);
        self.snapshot
            .status
            .remote
            .retain(|status| status.target_id != device_id);
        self.reconcile_remote_terminal_panes(device_id, &HashSet::new(), &HashSet::new());
        if self.snapshot.navigator.focused_device_id.as_deref() == Some(device_id) {
            self.snapshot.navigator.focused_device_id = Some(workspace::LOCAL_DEVICE_ID.to_owned());
            self.snapshot.ui_state.focused_device_id = None;
        }
    }

    /// A fresh connection attempt for a registered device that is not
    /// connected: the old coordinator and transports are retired and a new
    /// one starts, so the row reports this attempt rather than a cached one.
    /// Nothing on the host is touched, and the operator's device focus stays.
    pub(super) fn retry_remote_device(&mut self, device_id: &str) -> bool {
        let Some(registration) = self
            .snapshot
            .ui_state
            .device_registrations
            .iter()
            .find(|registration| registration.id == device_id)
            .cloned()
        else {
            self.set_error(
                "remote.unknown_target",
                format!("Device {device_id} is not registered"),
                false,
            );
            return true;
        };
        if self
            .snapshot
            .status
            .remote
            .iter()
            .any(|status| status.target_id == device_id && status.state == "ready")
        {
            self.set_error(
                "remote.retry_connected",
                format!("Device {device_id} is already connected"),
                false,
            );
            return true;
        }
        let focused_device = self.snapshot.navigator.focused_device_id.clone();
        let persisted_focus = self.snapshot.ui_state.focused_device_id.clone();
        self.disconnect_remote_device(device_id);
        self.snapshot.navigator.focused_device_id = focused_device;
        self.snapshot.ui_state.focused_device_id = persisted_focus;
        self.push_diagnostic(
            "device.retry",
            format!("Reconnecting SSH device {device_id}"),
        );
        self.connect_remote_device(&registration);
        self.refresh_device_snapshots();
        true
    }

    pub(crate) fn take_retired_remote_syncs(&mut self) -> Vec<session_sync::SessionSyncHandle> {
        std::mem::take(&mut self.retired_remote_syncs)
    }

    /// Every coordinator the runtime still owns, for the FFI layer to join
    /// before the runtime itself is dropped.
    pub(crate) fn take_remote_syncs(&mut self) -> Vec<session_sync::SessionSyncHandle> {
        let mut handles = self.take_retired_remote_syncs();
        handles.extend(
            self.remote_connections
                .values_mut()
                .filter_map(|connection| connection.sync.take()),
        );
        handles
    }

    /// Runs the staged capability test for a device on a worker thread and
    /// shows it as `running` until the report lands.
    pub(super) fn start_device_test(&mut self, device_id: &str) -> bool {
        let Some(connection) = self.remote_connections.get_mut(device_id) else {
            self.set_error(
                "device.not_connected",
                format!("Device {device_id} has no connection to test; remove and add it again"),
                false,
            );
            return true;
        };
        if connection.test_in_flight {
            self.set_error(
                "device.test_in_flight",
                format!("A connection test for {device_id} is already running"),
                false,
            );
            return true;
        }
        let Some(context) = self.worker_context.clone() else {
            self.set_error(
                "device.test_worker_unavailable",
                "The connection test worker is unavailable",
                true,
            );
            return true;
        };
        let client = Arc::clone(&connection.client);
        connection.test_in_flight = true;
        self.remote_device_tests.insert(
            device_id.to_owned(),
            DeviceTestSnapshot {
                state: "running".to_owned(),
                checked_at_unix_ms: None,
                stages: Vec::new(),
            },
        );
        self.push_diagnostic(
            "device.connection_test_requested",
            format!(
                "Connection test requested for {device_id}; SSH credentials remain outside hide"
            ),
        );
        let worker_device_id = device_id.to_owned();
        let spawned = thread::Builder::new()
            .name(format!("herdr-core-device-test-{device_id}"))
            .spawn(move || {
                let report = client.staged_capability_test("device-connection-test", false);
                let Some(runtime) = context.runtime.upgrade() else {
                    return;
                };
                let changed = match runtime.lock() {
                    Ok(mut guard) => guard.ingest_device_test(&worker_device_id, &report),
                    Err(_) => return,
                };
                drop(runtime);
                if changed {
                    context.notifier.notify();
                }
            });
        if let Err(error) = spawned {
            if let Some(connection) = self.remote_connections.get_mut(device_id) {
                connection.test_in_flight = false;
            }
            self.remote_device_tests.remove(device_id);
            self.set_error(
                "device.test_worker_unavailable",
                format!("The connection test could not start: {error}"),
                true,
            );
        }
        self.refresh_device_snapshots();
        true
    }

    pub(crate) fn ingest_device_test(
        &mut self,
        device_id: &str,
        report: &CapabilityReport,
    ) -> bool {
        let Some(connection) = self.remote_connections.get_mut(device_id) else {
            // Removed while the test ran; the answer has no row to land on.
            return false;
        };
        connection.test_in_flight = false;
        let stages = report
            .stages
            .iter()
            // The tunnel stage is an explicit probe the test does not run.
            .filter(|result| result.stage != crate::remote::RemoteStage::Tunnel)
            .map(|result| DeviceTestStageSnapshot {
                stage: result.stage.to_string(),
                state: match result.state {
                    CapabilityState::Pending => "pending",
                    CapabilityState::Passed => "passed",
                    CapabilityState::Failed { .. } => "failed",
                }
                .to_owned(),
                detail: result.detail.clone(),
            })
            .collect::<Vec<_>>();
        let passed = stages.iter().all(|stage| stage.state == "passed");
        self.remote_device_tests.insert(
            device_id.to_owned(),
            DeviceTestSnapshot {
                state: if passed { "passed" } else { "failed" }.to_owned(),
                checked_at_unix_ms: Some(unix_milliseconds()),
                stages,
            },
        );
        self.push_diagnostic(
            if passed {
                "device.connection_test_passed"
            } else {
                "device.connection_test_failed"
            },
            format!("Connection test for {device_id} finished"),
        );
        self.refresh_device_snapshots();
        true
    }

    /// Reads each remote device row off its remote status and last test. The
    /// rows are rebuilt with every catalog, so this is the one place their
    /// state comes from.
    pub(crate) fn refresh_device_snapshots(&mut self) -> bool {
        let statuses = &self.snapshot.status.remote;
        let tests = &self.remote_device_tests;
        let mut changed = false;
        for device in &mut self.snapshot.navigator.devices {
            if device.kind != "remote" {
                continue;
            }
            let status = statuses.iter().find(|status| status.target_id == device.id);
            let (state, message) = match status {
                Some(status) if status.state == "connected" => ("ready", None),
                Some(status) => ("unavailable", status.message.clone()),
                None => ("unavailable", None),
            };
            let test = tests.get(&device.id).cloned();
            if device.state != state || device.message != message || device.test != test {
                device.state = state.to_owned();
                device.message = message;
                device.test = test;
                changed = true;
            }
        }
        changed
    }
}
