//! Every node's terminals behind one [`TerminalRoutes`], as the core and the
//! screen on this machine reach them (PRD core-host-node-terminal D-02,
//! D-11). This machine's panes go to its own [`super::Service`] by their own
//! ids; a device's panes, which the core names `remote:<device>:pane:<id>`,
//! go to the device's node inside its link, by the device's own ids.
//!
//! Nothing here holds a byte for long: a key, a control or a frame takes
//! the routing lock for a map lookup and goes on.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use hide_node_link::terminal::{
    GridSize, KeyTarget, PaneTerminalState, ReportSink, TerminalControl, TerminalNode,
    TerminalReport, TerminalRoutes, device_pane_id, device_pane_prefix,
};
use serde_json::json;

use super::OutputSink;
use super::device::DeviceSink;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Where one pane's terminal is.
enum Route {
    Local,
    Device {
        node: Arc<dyn TerminalNode>,
        pane: String,
    },
    /// A device pane whose device has no link now.
    Unrouted,
}

#[derive(Default)]
struct Routes {
    devices: HashMap<String, Arc<dyn TerminalNode>>,
    /// The panes on screen, as the core last said, by the core's ids.
    shown: Vec<String>,
    /// Which device a paste's held input is on; a paste on this machine
    /// has none.
    intents: HashMap<String, Option<String>>,
    /// The device panes whose output reached the screen's hub, so a device
    /// that leaves takes its panes' output with it.
    device_panes: HashMap<String, HashSet<String>>,
}

/// This machine's terminals and its devices', routed by pane.
pub struct Router {
    local: Arc<dyn TerminalNode>,
    hub: Arc<dyn OutputSink>,
    reports: Arc<dyn ReportSink>,
    routes: Mutex<Routes>,
}

impl Router {
    /// `local` is this machine's service, which writes to `hub` and reports
    /// to `reports` itself; a device's output and reports reach them through
    /// the router ([`DeviceSink`]).
    pub fn new(
        local: Arc<dyn TerminalNode>,
        hub: Arc<dyn OutputSink>,
        reports: Arc<dyn ReportSink>,
    ) -> Self {
        Self {
            local,
            hub,
            reports,
            routes: Mutex::default(),
        }
    }

    /// The device a scoped pane id names, and the device's own id for it.
    fn device_of<'a>(routes: &Routes, pane: &'a str) -> Option<(String, &'a str)> {
        let rest = pane.strip_prefix("remote:")?;
        // A device id is matched against the devices the router knows
        // first, so one whose id holds `:pane:` still routes.
        if let Some((device, source)) = routes.devices.keys().find_map(|device| {
            pane.strip_prefix(&device_pane_prefix(device))
                .map(|source| (device.clone(), source))
        }) {
            return Some((device, source));
        }
        let (device, source) = rest.split_once(":pane:")?;
        Some((device.to_owned(), source))
    }

    fn route(&self, pane: &str) -> Route {
        let routes = lock(&self.routes);
        match Self::device_of(&routes, pane) {
            None => Route::Local,
            Some((device, source)) => match routes.devices.get(&device) {
                Some(node) => Route::Device {
                    node: Arc::clone(node),
                    pane: source.to_owned(),
                },
                None => Route::Unrouted,
            },
        }
    }

    fn unrouted_attach(&self, pane: String) {
        self.reports.report(TerminalReport::State {
            pane,
            state: PaneTerminalState {
                state: "unavailable".to_owned(),
                mode: None,
                generation: 0,
                attempt: 0,
                message: Some(
                    "This device's helper is not connected, so its terminal cannot attach; it attaches when the helper connects"
                        .to_owned(),
                ),
                exit_category: Some("device_unavailable".to_owned()),
                retry_decision: "manual".to_owned(),
                last_attempt_at_unix_ms: None,
            },
        });
    }

    /// Sends each device the panes of its own on screen.
    fn shown_to(routes: &Routes, device: &str, node: &dyn TerminalNode) {
        let prefix = device_pane_prefix(device);
        let panes = routes
            .shown
            .iter()
            .filter_map(|pane| pane.strip_prefix(&prefix))
            .map(str::to_owned)
            .collect();
        node.control(TerminalControl::Shown { panes });
    }
}

impl TerminalNode for Router {
    fn control(&self, mut control: TerminalControl) {
        match &mut control {
            TerminalControl::Shown { panes } => {
                let mut routes = lock(&self.routes);
                routes.shown = panes.clone();
                let local = panes
                    .iter()
                    .filter(|pane| Self::device_of(&routes, pane).is_none())
                    .cloned()
                    .collect();
                for (device, node) in &routes.devices {
                    Self::shown_to(&routes, device, node.as_ref());
                }
                drop(routes);
                self.local.control(TerminalControl::Shown { panes: local });
                return;
            }
            TerminalControl::RequestOpen { .. }
            | TerminalControl::RequestResolve { .. }
            | TerminalControl::RequestDiscard { .. } => {
                // The core opens creation requests for this machine's panes.
                self.local.control(control);
                return;
            }
            TerminalControl::AttachmentRefuse { intent }
            | TerminalControl::AttachmentDeliver { intent, .. } => {
                let intent = intent.clone();
                let routes = lock(&self.routes);
                let node = match routes.intents.get(&intent).cloned().flatten() {
                    None => Some(Arc::clone(&self.local)),
                    Some(device) => routes.devices.get(&device).cloned(),
                };
                drop(routes);
                if let Some(node) = node {
                    node.control(control);
                }
                return;
            }
            TerminalControl::AttachmentRelease { intent } => {
                let mut routes = lock(&self.routes);
                let device = routes.intents.remove(intent.as_str());
                let node = match device.flatten() {
                    None => Some(Arc::clone(&self.local)),
                    Some(device) => routes.devices.get(&device).cloned(),
                };
                drop(routes);
                if let Some(node) = node {
                    node.control(control);
                }
                return;
            }
            _ => {}
        }
        let Some(scoped) = control.pane_mut().map(|pane| pane.clone()) else {
            return;
        };
        if let TerminalControl::AttachmentHold { intent, .. } = &control {
            let mut routes = lock(&self.routes);
            let device = Self::device_of(&routes, &scoped).map(|(device, _)| device);
            routes.intents.insert(intent.clone(), device);
        }
        match self.route(&scoped) {
            Route::Local => self.local.control(control),
            Route::Device { node, pane } => {
                if matches!(
                    control,
                    TerminalControl::Release { .. } | TerminalControl::Forget { .. }
                ) {
                    self.hub.forget(&scoped);
                    let mut routes = lock(&self.routes);
                    for panes in routes.device_panes.values_mut() {
                        panes.remove(&scoped);
                    }
                }
                if let Some(target) = control.pane_mut() {
                    *target = pane;
                }
                node.control(control);
            }
            Route::Unrouted => match control {
                TerminalControl::Attach { .. } => self.unrouted_attach(scoped),
                TerminalControl::Release { .. } | TerminalControl::Forget { .. } => {
                    self.hub.forget(&scoped)
                }
                _ => {}
            },
        }
    }

    fn key(&self, target: KeyTarget, bytes: Vec<u8>, typed_at_unix_ms: u64) {
        let KeyTarget::Pane(scoped) = target else {
            self.local.key(target, bytes, typed_at_unix_ms);
            return;
        };
        match self.route(&scoped) {
            Route::Local => self
                .local
                .key(KeyTarget::Pane(scoped), bytes, typed_at_unix_ms),
            Route::Device { node, pane } => {
                node.key(KeyTarget::Pane(pane), bytes, typed_at_unix_ms)
            }
            // A key for a device with no link is refused, never kept for
            // a later link (D-19, B17).
            Route::Unrouted => self.reports.report(TerminalReport::Error {
                pane: scoped,
                kind: "terminal.device_disconnected".to_owned(),
                message: "This device is not connected, so the key was not sent".to_owned(),
            }),
        }
    }

    fn view(&self, pane: &str, size: GridSize, new_view: bool) {
        match self.route(pane) {
            Route::Local => self.local.view(pane, size, new_view),
            Route::Device { node, pane } => node.view(&pane, size, new_view),
            Route::Unrouted => {}
        }
    }

    fn redraw(&self, pane: &str) {
        match self.route(pane) {
            Route::Local => self.local.redraw(pane),
            Route::Device { node, pane } => node.redraw(&pane),
            Route::Unrouted => {}
        }
    }
}

impl TerminalRoutes for Router {
    fn install_device(&self, device: &str, node: Arc<dyn TerminalNode>) {
        let mut routes = lock(&self.routes);
        Self::shown_to(&routes, device, node.as_ref());
        let replaced = routes.devices.insert(device.to_owned(), node);
        drop(routes);
        // The replaced link's writer ends once it is dropped, outside the
        // routing lock.
        drop(replaced);
        crate::diagnostic!(json!({
            "component": "terminal_router",
            "kind": "terminal.device_installed",
            "device": device,
        }));
    }

    fn remove_device(&self, device: &str) {
        let mut routes = lock(&self.routes);
        let removed = routes.devices.remove(device);
        let panes = routes.device_panes.remove(device).unwrap_or_default();
        routes
            .intents
            .retain(|_, holder| holder.as_deref() != Some(device));
        drop(routes);
        for pane in panes {
            self.hub.forget(&pane);
        }
        if removed.is_some() {
            crate::diagnostic!(json!({
                "component": "terminal_router",
                "kind": "terminal.device_removed",
                "device": device,
            }));
        }
        drop(removed);
    }
}

impl DeviceSink for Router {
    fn output(&self, device: &str, pane: &str, bytes: &[u8], full: bool) {
        let scoped = device_pane_id(device, pane);
        {
            let mut routes = lock(&self.routes);
            if !routes.devices.contains_key(device) {
                return;
            }
            routes
                .device_panes
                .entry(device.to_owned())
                .or_default()
                .insert(scoped.clone());
        }
        self.hub.output(&scoped, bytes, full);
    }

    fn report(&self, device: &str, mut report: TerminalReport) {
        if let Some(pane) = report.pane_mut() {
            *pane = device_pane_id(device, pane);
        }
        self.reports.report(report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder {
        controls: Mutex<Vec<TerminalControl>>,
        keys: Mutex<Vec<(KeyTarget, Vec<u8>)>>,
    }

    impl TerminalNode for Recorder {
        fn control(&self, control: TerminalControl) {
            self.controls.lock().unwrap().push(control);
        }
        fn key(&self, target: KeyTarget, bytes: Vec<u8>, _typed_at_unix_ms: u64) {
            self.keys.lock().unwrap().push((target, bytes));
        }
        fn view(&self, _pane: &str, _size: GridSize, _new_view: bool) {}
        fn redraw(&self, _pane: &str) {}
    }

    #[derive(Default)]
    struct Hub {
        output: Mutex<Vec<(String, Vec<u8>)>>,
        forgotten: Mutex<Vec<String>>,
    }

    impl OutputSink for Hub {
        fn output(&self, pane: &str, bytes: &[u8], _full: bool) {
            self.output
                .lock()
                .unwrap()
                .push((pane.to_owned(), bytes.to_vec()));
        }
        fn forget(&self, pane: &str) {
            self.forgotten.lock().unwrap().push(pane.to_owned());
        }
    }

    #[derive(Default)]
    struct Reports(Mutex<Vec<TerminalReport>>);

    impl ReportSink for Reports {
        fn report(&self, report: TerminalReport) {
            self.0.lock().unwrap().push(report);
        }
    }

    fn router() -> (Router, Arc<Recorder>, Arc<Hub>, Arc<Reports>) {
        let local = Arc::new(Recorder::default());
        let hub = Arc::new(Hub::default());
        let reports = Arc::new(Reports::default());
        let router = Router::new(
            Arc::clone(&local) as Arc<dyn TerminalNode>,
            Arc::clone(&hub) as Arc<dyn OutputSink>,
            Arc::clone(&reports) as Arc<dyn ReportSink>,
        );
        (router, local, hub, reports)
    }

    #[test]
    fn a_device_pane_reaches_its_device_by_the_devices_own_id() {
        let (router, local, hub, reports) = router();
        let mini = Arc::new(Recorder::default());
        router.control(TerminalControl::Shown {
            panes: vec!["w1:p1".into(), "remote:mini:pane:w2:p3".into()],
        });
        router.install_device("mini", Arc::clone(&mini) as Arc<dyn TerminalNode>);
        assert_eq!(
            mini.controls.lock().unwrap().as_slice(),
            [TerminalControl::Shown {
                panes: vec!["w2:p3".into()]
            }]
        );
        router.key(
            KeyTarget::Pane("remote:mini:pane:w2:p3".into()),
            b"a".to_vec(),
            1,
        );
        router.key(KeyTarget::Pane("w1:p1".into()), b"b".to_vec(), 1);
        assert_eq!(
            mini.keys.lock().unwrap().as_slice(),
            [(KeyTarget::Pane("w2:p3".into()), b"a".to_vec())]
        );
        assert_eq!(
            local.keys.lock().unwrap().as_slice(),
            [(KeyTarget::Pane("w1:p1".into()), b"b".to_vec())]
        );
        // Output and reports from the device come back under the core's ids.
        router.output("mini", "w2:p3", b"frame", true);
        assert_eq!(
            hub.output.lock().unwrap().as_slice(),
            [("remote:mini:pane:w2:p3".to_owned(), b"frame".to_vec())]
        );
        DeviceSink::report(
            &router,
            "mini",
            TerminalReport::FirstFrame {
                pane: "w2:p3".into(),
                generation: 2,
            },
        );
        assert_eq!(
            reports.0.lock().unwrap().as_slice(),
            [TerminalReport::FirstFrame {
                pane: "remote:mini:pane:w2:p3".into(),
                generation: 2
            }]
        );
    }

    #[test]
    fn a_device_without_a_link_refuses_keys_and_reads_unavailable() {
        let (router, _local, hub, reports) = router();
        let mini = Arc::new(Recorder::default());
        router.install_device("mini", Arc::clone(&mini) as Arc<dyn TerminalNode>);
        router.output("mini", "w2:p3", b"frame", true);
        router.remove_device("mini");
        assert_eq!(
            hub.forgotten.lock().unwrap().as_slice(),
            ["remote:mini:pane:w2:p3"]
        );
        router.key(
            KeyTarget::Pane("remote:mini:pane:w2:p3".into()),
            b"a".to_vec(),
            1,
        );
        router.control(TerminalControl::Attach {
            pane: "remote:mini:pane:w2:p3".into(),
            size: None,
            manual: false,
        });
        let reports = reports.0.lock().unwrap();
        assert!(
            matches!(&reports[0], TerminalReport::Error { kind, .. } if kind == "terminal.device_disconnected")
        );
        assert!(
            matches!(&reports[1], TerminalReport::State { state, .. } if state.state == "unavailable")
        );
        // Output a removed device's late reader sends reaches no screen.
        drop(reports);
        router.output("mini", "w2:p3", b"late", false);
        assert_eq!(hub.output.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_pastes_later_controls_follow_its_pane_to_its_device() {
        let (router, local, _hub, _reports) = router();
        let mini = Arc::new(Recorder::default());
        router.install_device("mini", Arc::clone(&mini) as Arc<dyn TerminalNode>);
        router.control(TerminalControl::AttachmentHold {
            pane: "remote:mini:pane:w2:p3".into(),
            intent: "paste-1".into(),
        });
        router.control(TerminalControl::AttachmentDeliver {
            intent: "paste-1".into(),
            generation: 3,
            paste: "'/tmp/a.png' ".into(),
        });
        router.control(TerminalControl::AttachmentRelease {
            intent: "paste-1".into(),
        });
        let controls = mini.controls.lock().unwrap();
        assert!(
            matches!(&controls[1], TerminalControl::AttachmentHold { pane, .. } if pane == "w2:p3")
        );
        assert!(matches!(
            &controls[2],
            TerminalControl::AttachmentDeliver { .. }
        ));
        assert!(matches!(
            &controls[3],
            TerminalControl::AttachmentRelease { .. }
        ));
        assert!(local.controls.lock().unwrap().is_empty());
    }
}
