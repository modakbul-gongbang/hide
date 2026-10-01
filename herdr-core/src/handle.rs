//! The safe handle hided drives the runtime through: create it, dispatch
//! events into it, read snapshot deltas out of it, and be told when it
//! changed. Every call belongs to the thread that created the handle; hided
//! keeps that thread and forwards its Axum workers' requests over a channel.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, ThreadId};

use crate::model::CoreOptions;
use crate::runtime::{Runtime, validate_options};
use crate::{environment, live};

type Callback = Arc<dyn Fn() + Send + Sync>;

/// Thread-safe handle that fires the registered change callback. Cloned into
/// live worker threads so PTY and session-sync output can wake the daemon's
/// snapshot reader.
///
/// Announcements coalesce. The reader answers one by taking the whole delta,
/// so every change between an announcement and the read that answers it is
/// already carried by that read; announcing each one separately bought the
/// reader one wake and one turn waiting on the runtime mutex per PTY chunk.
#[derive(Clone)]
pub struct ChangeNotifier {
    registration: Arc<Mutex<Option<Callback>>>,
    /// True from an announcement until the read that answers it begins.
    announced: Arc<AtomicBool>,
}

impl ChangeNotifier {
    fn new() -> Self {
        Self {
            registration: Arc::new(Mutex::new(None)),
            announced: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn notify(&self) {
        let registration = lock_recover(&self.registration).clone();
        // Latching with nobody listening would swallow the first real
        // announcement, so an unregistered notifier stays silent and unlatched.
        let Some(registration) = registration else {
            return;
        };
        if self.announced.swap(true, Ordering::AcqRel) {
            return;
        }
        // The callback runs on whichever worker announced; a panic in it must
        // not take a PTY or session-sync thread down with it.
        let _ = catch_unwind(AssertUnwindSafe(|| registration()));
    }

    /// Called before the reader takes the runtime lock, never after.
    ///
    /// Clearing first means a change that lands while the delta is being taken
    /// announces itself again; the reader may then run once for nothing, which
    /// costs a read. Clearing afterwards would read that announcement as
    /// already delivered and leave the change on screen-invisible state until
    /// something else happened to notify.
    fn clear_announcement(&self) {
        self.announced.store(false, Ordering::Release);
    }

    fn set_callback(&self, registration: Option<Callback>) {
        *lock_recover(&self.registration) = registration;
    }

    #[cfg(test)]
    pub(crate) fn noop() -> Self {
        Self::new()
    }
}

pub struct Core {
    _terminal_maintenance: Option<crate::terminal_recovery::Maintenance>,
    _changes: Option<crate::changes::ChangesPump>,
    _kit: Option<crate::kit::KitPump>,
    _session_sync: Option<crate::session_sync::SessionSyncHandle>,
    _session_search: Option<crate::runtime::session_search::SearchWorker>,
    labels: Option<Arc<crate::labels::LabelServices>>,
    runtime: Arc<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    owner_thread: ThreadId,
}

impl Drop for Core {
    fn drop(&mut self) {
        // Reserve cancellation before any coordinator shutdown can wait: a
        // completing attachment must not enqueue input during destruction.
        let attachment_worker = { lock_recover(&self.runtime).take_attachment_worker() };
        self._session_search.take();
        self._terminal_maintenance.take();
        self._changes.take();
        self._session_sync.take();
        // Taken under the lock, joined outside it: a coordinator's last act is
        // to lock the runtime, so a join under the lock never returns.
        let remote_syncs = { lock_recover(&self.runtime).take_remote_syncs() };
        drop(remote_syncs);
        // Every worker has stopped handing in analyses; the one running is
        // cancelled, which ends its provider child (labels-in-hided B22).
        if let Some(labels) = self.labels.take() {
            labels.analyzer.shutdown();
        }
        if let Some(worker) = attachment_worker {
            let _ = worker.join();
        }
        // A layout or View tab changed just before quitting is on disk before
        // the process goes (B19).
        let views_worker = { lock_recover(&self.runtime).take_workspace_views_save_worker() };
        if let Some(worker) = views_worker
            && worker.join().is_err()
        {
            crate::diagnostic!(
                serde_json::json!({"component":"workspace_views", "kind":"save.join_failed"})
            );
        }
        let worker = { lock_recover(&self.runtime).take_state_save_worker() };
        if let Some(worker) = worker
            && worker.join().is_err()
        {
            crate::diagnostic!(
                serde_json::json!({"component":"ui_state", "kind":"save.join_failed"})
            );
        }
    }
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn check_owner_thread(core: &Core, operation: &str) -> bool {
    if thread::current().id() == core.owner_thread {
        return true;
    }
    lock_recover(&core.runtime).set_error(
        "core.wrong_thread",
        format!("{operation} must run on the thread that created herdr-core"),
        false,
    );
    false
}

fn notify_change(core: &Core) {
    core.notifier.notify();
}

/// `create`, `dispatch_bytes`, `snapshot_delta`, `on_change`, and `Drop`
/// belong to the thread that created the value. Axum workers talk to it
/// through an owner-thread channel in `hided`.
impl Core {
    pub fn create(options: CoreOptions) -> Option<Box<Self>> {
        if validate_options(&options).is_err() {
            return None;
        }
        let mut options = options;
        let environment = environment::read_and_validate();
        let environment_home = environment.home_path.clone();
        let usage_paths = crate::usage::UsagePaths {
            home: environment.home_path.clone(),
            claude_cwd: std::path::Path::new(&options.app_state_path)
                .parent()
                .filter(|directory| !directory.as_os_str().is_empty())
                .map(std::path::Path::to_path_buf),
            codex_home: environment.codex_home.clone(),
        };
        if options.herdr_socket_path.is_some()
            && let Some(path) = environment.herdr_socket_path_override.as_ref()
        {
            options.herdr_socket_path = Some(path.clone());
        }
        #[cfg(not(test))]
        if let Err(error) =
            crate::diagnostics::install(std::path::Path::new(&options.app_state_path))
        {
            std::eprintln!(
                "{}",
                serde_json::json!({"kind": "diagnostics.open_failed", "message": error.to_string()})
            );
        }
        let runtime = Arc::new(Mutex::new(Runtime::new(options.clone(), environment)));
        let notifier = ChangeNotifier::new();
        lock_recover(&runtime).install_worker_context(Arc::downgrade(&runtime), notifier.clone());
        // Before any coordinator starts, because each builds its label worker
        // on these, and before the kit runs, because the plugin state the
        // store imports once is what the kit's retirement deletes.
        let labels = match crate::labels::LabelServices::start(
            std::path::Path::new(&options.app_state_path)
                .parent()
                .filter(|directory| !directory.as_os_str().is_empty()),
            environment_home.clone(),
            Arc::downgrade(&runtime),
        ) {
            Ok(services) => {
                let services = Arc::new(services);
                lock_recover(&runtime).install_label_services(Arc::clone(&services));
                Some(services)
            }
            Err(message) => {
                crate::diagnostic!(
                    serde_json::json!({"component":"labels","kind":"services.start_failed","message":message})
                );
                None
            }
        };
        let session_search = match crate::runtime::session_search::SearchWorker::spawn(
            Arc::downgrade(&runtime),
            notifier.clone(),
        ) {
            Ok(worker) => {
                worker.install(&mut lock_recover(&runtime));
                Some(worker)
            }
            Err(message) => {
                crate::diagnostic!(
                    serde_json::json!({"component":"session_search","kind":"worker.start_failed","message":message})
                );
                None
            }
        };
        let session_sync = if let Some(socket_path) = options.herdr_socket_path.as_deref() {
            live::install(
                &runtime,
                notifier.clone(),
                socket_path,
                options.herdr_bin_path.as_deref(),
                usage_paths,
            )
        } else {
            None
        };
        if lock_recover(&runtime).connect_registered_devices() {
            notifier.notify();
        }
        let maintenance = match crate::terminal_recovery::Maintenance::spawn(
            Arc::downgrade(&runtime),
            notifier.clone(),
        ) {
            Ok(handle) => Some(handle),
            Err(error) => {
                lock_recover(&runtime).set_error(
                    "terminal.recovery_unavailable",
                    error.to_string(),
                    true,
                );
                None
            }
        };
        // History reads through each checkout's own host, so it runs whether
        // or not this machine has a Herdr session.
        let changes =
            match crate::changes::ChangesPump::spawn(Arc::downgrade(&runtime), notifier.clone()) {
                Ok(pump) => Some(pump),
                Err(error) => {
                    lock_recover(&runtime).set_error(
                        "changes.reader_unavailable",
                        error.to_string(),
                        true,
                    );
                    None
                }
            };
        // This Mac's install kit runs whether or not Herdr answers; only the
        // labels plugin needs it (PRD device-parity B1, B6).
        let herdr_socket = options
            .herdr_socket_path
            .as_ref()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                environment_home
                    .as_ref()
                    .map(|home| home.join(".config/herdr/herdr.sock"))
            })
            .unwrap_or_default();
        let kit = match crate::kit::local_target(
            options.kit_dir.as_deref(),
            environment_home,
            herdr_socket,
            std::sync::Arc::default(),
        ) {
            Ok(target) => {
                lock_recover(&runtime).queue_local_kit_launch();
                match crate::kit::KitPump::spawn(Arc::downgrade(&runtime), notifier.clone(), target)
                {
                    Ok(pump) => Some(pump),
                    Err(error) => {
                        lock_recover(&runtime).set_local_kit_unavailable(&format!(
                            "Hide could not start its installer: {error}"
                        ));
                        None
                    }
                }
            }
            Err(reason) => {
                lock_recover(&runtime).set_local_kit_unavailable(&reason);
                None
            }
        };
        Some(Box::new(Core {
            _terminal_maintenance: maintenance,
            _changes: changes,
            _kit: kit,
            _session_sync: session_sync,
            _session_search: session_search,
            labels,
            runtime,
            notifier,
            owner_thread: thread::current().id(),
        }))
    }

    pub fn dispatch_bytes(&self, bytes: &[u8]) -> bool {
        if !check_owner_thread(self, "dispatch") {
            notify_change(self);
            return false;
        }
        let (changed, retired_syncs) = {
            let mut runtime = lock_recover(&self.runtime);
            let changed = runtime.dispatch_json(bytes);
            (changed, runtime.take_retired_remote_syncs())
        };
        drop(retired_syncs);
        if changed {
            notify_change(self);
        }
        changed
    }

    /// Where each device's file work runs, handed over once by the daemon.
    pub fn set_file_roots(&self, roots: crate::files::FileRoots) {
        if check_owner_thread(self, "set_file_roots")
            && lock_recover(&self.runtime).set_file_roots(roots)
        {
            notify_change(self);
        }
    }

    /// Where a device's file work runs, for the daemon's own
    /// requests that answer outside the snapshot (the Explorer's listing).
    /// Asking may start the device's helper, which the snapshot announces.
    pub fn device_channel(
        &self,
        device_id: &str,
    ) -> Result<std::sync::Arc<dyn crate::host_access::HostChannel>, String> {
        if !check_owner_thread(self, "device_channel") {
            return Err("the core was called off its owner thread".to_owned());
        }
        let result = lock_recover(&self.runtime).device_channel(device_id);
        // Only a refusal can have started a helper connection.
        if result.is_err() {
            notify_change(self);
        }
        result
    }

    /// The Herdr API connection the core holds for a connected SSH device;
    /// `None` when the device is not connected. The daemon reads and writes
    /// a device pane through it without joining the attach set.
    pub fn remote_herdr_api(
        &self,
        device_id: &str,
    ) -> Option<std::sync::Arc<dyn hide_herdr_client::ApiConnector>> {
        if !check_owner_thread(self, "remote_herdr_api") {
            return None;
        }
        lock_recover(&self.runtime).remote_herdr_api(device_id)
    }

    /// Daemon-only read of consented, connected SSH routes. Opening a route
    /// happens after this call has released the runtime lock.
    pub fn workspace_remote_routes(&self) -> Vec<crate::WorkspaceRemoteRoute> {
        if !check_owner_thread(self, "workspace_remote_routes") {
            return Vec::new();
        }
        lock_recover(&self.runtime).workspace_remote_routes()
    }

    pub fn browser_route_source(
        &self,
        device_id: &str,
        checkout_path: &str,
        view_id: &str,
        load: u64,
    ) -> Option<crate::workspace_control::BrowserRouteSource> {
        if !check_owner_thread(self, "browser_route_source") {
            return None;
        }
        lock_recover(&self.runtime).browser_route_source(device_id, checkout_path, view_id, load)
    }

    /// Daemon-only pane query. The daemon validates the process that asked;
    /// the core then resolves current pane membership at the point of use.
    /// Only owned result data leaves the runtime lock.
    pub fn workspace_control_query(
        &self,
        device_id: &str,
        pane_id: &str,
        query: crate::workspace_control::Query,
    ) -> Result<crate::workspace_control::QueryResult, crate::workspace_control::Refusal> {
        if !check_owner_thread(self, "workspace_control_query") {
            return Err(crate::workspace_control::Refusal {
                reason: "core_unavailable",
                next_action: "Reconnect Hide and retry",
            });
        }
        lock_recover(&self.runtime).workspace_control_query(device_id, pane_id, query)
    }

    /// A pane-scoped View action runs as one core transition. The daemon
    /// attests the caller, and the core rechecks current membership.
    pub fn workspace_control_prepare_action(
        &self,
        device_id: &str,
        pane_id: &str,
        expected: &crate::workspace_control::Context,
        request_id: &str,
        action: &crate::workspace_control::Action,
    ) -> Result<crate::workspace_control::ActionPreparation, crate::workspace_control::Refusal>
    {
        if !check_owner_thread(self, "workspace_control_prepare_action") {
            return Err(crate::workspace_control::Refusal {
                reason: "core_unavailable",
                next_action: "Reconnect Hide and retry",
            });
        }
        lock_recover(&self.runtime)
            .workspace_control_prepare_action(device_id, pane_id, expected, request_id, action)
    }

    pub fn workspace_control_action(
        &self,
        device_id: &str,
        pane_id: &str,
        expected: &crate::workspace_control::Context,
        request_id: &str,
        action: crate::workspace_control::Action,
        material: Result<
            Option<crate::workspace_control::ActionMaterial>,
            crate::workspace_control::Refusal,
        >,
    ) -> Result<crate::workspace_control::ActionResult, crate::workspace_control::Refusal> {
        if !check_owner_thread(self, "workspace_control_action") {
            return Err(crate::workspace_control::Refusal {
                reason: "core_unavailable",
                next_action: "Reconnect Hide and retry",
            });
        }
        let result = lock_recover(&self.runtime)
            .workspace_control_action(device_id, pane_id, expected, request_id, action, material);
        if result.as_ref().is_ok_and(|result| result.changed) {
            notify_change(self);
        }
        result
    }

    pub fn snapshot_delta(&self, have_revision: u64, have_terminal_sequence: u64) -> Vec<u8> {
        if !check_owner_thread(self, "snapshot") {
            notify_change(self);
            return Vec::new();
        }
        self.notifier.clear_announcement();
        let payload = {
            let mut runtime = lock_recover(&self.runtime);
            runtime.snapshot_delta_payload(have_revision, have_terminal_sequence)
        };
        crate::runtime::serialize_snapshot_delta(&payload).unwrap_or_default()
    }

    pub fn on_change<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        if !check_owner_thread(self, "on_change") {
            notify_change(self);
            return;
        }
        self.notifier.set_callback(Some(Arc::new(callback)));
    }

    pub fn clear_on_change(&self) {
        if !check_owner_thread(self, "on_change") {
            notify_change(self);
            return;
        }
        self.notifier.set_callback(None);
    }
}
