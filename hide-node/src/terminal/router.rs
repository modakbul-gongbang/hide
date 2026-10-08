//! Every node's terminals behind one [`TerminalRoutes`], as the core and the
//! screen on this machine reach them (PRD core-host-node-terminal D-02,
//! D-11). This machine's panes go to its own [`super::Service`] by their own
//! ids; a device's panes, which the core names `remote:<device>:pane:<id>`,
//! go to the device's node inside its link, by the device's own ids.
//!
//! Nothing here holds a byte for long: a key, a control or a frame takes
//! the routing lock for a map lookup and goes on.
//!
//! A device's node is another machine's program, so what it sends up is
//! taken only where the core asked for it: output for a pane the core
//! attached on that device, a report about such a pane or one on screen, and
//! a paste's outcome for a paste held on that device. Its word that a key
//! moved the keyboard counts only for the pane the last screen key went to.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use hide_node_link::terminal::{
    GridSize, KeyTarget, PaneTerminalState, ReportSink, TerminalControl, TerminalNode,
    TerminalReport, TerminalRoutes, device_pane_id, device_pane_prefix,
};
use serde_json::json;

use super::OutputSink;
use super::device::DeviceSink;
use super::input::Refusals;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Where one pane's terminal is.
enum Route {
    Local,
    Device {
        device: String,
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
    /// The device panes the core attached, by device and by the core's ids:
    /// only their output reaches the screen's hub, and a device that leaves
    /// takes their output with it.
    attached: HashMap<String, HashSet<String>>,
    /// The pane the last screen key went to, by the core's ids.
    keyed: Option<String>,
    /// Lines each device sent that were not taken, since the log last said.
    unadmitted: HashMap<String, u64>,
    refusals: Refusals,
}

impl Routes {
    fn attached(&self, device: &str, pane: &str) -> bool {
        self.attached
            .get(device)
            .is_some_and(|panes| panes.contains(pane))
    }

    /// Counts a line `device` sent that was not taken, and says how many
    /// there were once a window, for the log.
    fn unadmitted(&mut self, device: &str) -> Option<u64> {
        let count = self.unadmitted.entry(device.to_owned()).or_default();
        *count += 1;
        let count = *count;
        self.refusals
            .report(device, "terminal.device_line_unadmitted", Instant::now())
            .then(|| {
                self.unadmitted.remove(device);
                count
            })
    }
}

fn log_unadmitted(device: &str, what: &str, lines: Option<u64>) {
    if let Some(lines) = lines {
        crate::diagnostic!(json!({
            "component": "terminal_router",
            "kind": "terminal.device_line_unadmitted",
            "device": device,
            "line": what,
            "lines": lines,
        }));
    }
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
        Self::route_in(&lock(&self.routes), pane)
    }

    fn route_in(routes: &Routes, pane: &str) -> Route {
        match Self::device_of(routes, pane) {
            None => Route::Local,
            Some((device, source)) => match routes.devices.get(&device) {
                Some(node) => Route::Device {
                    node: Arc::clone(node),
                    pane: source.to_owned(),
                    device,
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
            Route::Device { device, node, pane } => {
                match control {
                    TerminalControl::Attach { .. } => {
                        lock(&self.routes)
                            .attached
                            .entry(device)
                            .or_default()
                            .insert(scoped);
                    }
                    TerminalControl::Release { .. } | TerminalControl::Forget { .. } => {
                        if let Some(panes) = lock(&self.routes).attached.get_mut(&device) {
                            panes.remove(&scoped);
                        }
                        self.hub.forget(&scoped);
                    }
                    _ => {}
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
        let route = {
            let mut routes = lock(&self.routes);
            if routes.keyed.as_deref() != Some(scoped.as_str()) {
                routes.keyed = Some(scoped.clone());
            }
            let route = Self::route_in(&routes, &scoped);
            // A key for a device with no link is refused, never kept for a
            // later link (D-19, B17), and a flood of them is refused once a
            // window, like a node's own refusals.
            if matches!(route, Route::Unrouted)
                && !routes
                    .refusals
                    .report(&scoped, "terminal.device_disconnected", Instant::now())
            {
                return;
            }
            route
        };
        match route {
            Route::Local => self
                .local
                .key(KeyTarget::Pane(scoped), bytes, typed_at_unix_ms),
            Route::Device { node, pane, .. } => {
                node.key(KeyTarget::Pane(pane), bytes, typed_at_unix_ms)
            }
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
            Route::Device { node, pane, .. } => node.view(&pane, size, new_view),
            Route::Unrouted => {}
        }
    }

    fn redraw(&self, pane: &str) {
        match self.route(pane) {
            Route::Local => self.local.redraw(pane),
            Route::Device { node, pane, .. } => node.redraw(&pane),
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
        let panes = routes.attached.remove(device).unwrap_or_default();
        routes.unadmitted.remove(device);
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
            if !routes.attached(device, &scoped) {
                let lines = routes.unadmitted(device);
                drop(routes);
                log_unadmitted(device, "output", lines);
                return;
            }
        }
        self.hub.output(&scoped, bytes, full);
    }

    fn report(&self, device: &str, mut report: TerminalReport) {
        if let Some(pane) = report.pane_mut() {
            *pane = device_pane_id(device, pane);
        }
        let mut routes = lock(&self.routes);
        let admitted = match &mut report {
            // The core opens creation requests for this machine's panes only.
            TerminalReport::RequestDiscarded { .. } => false,
            TerminalReport::AttachmentInput { intent, .. }
            | TerminalReport::AttachmentDelivered { intent, .. } => routes
                .intents
                .get(intent.as_str())
                .is_some_and(|holder| holder.as_deref() == Some(device)),
            TerminalReport::Input { pane, focus, .. } => {
                *focus &= routes.keyed.as_deref() == Some(pane.as_str());
                routes.attached(device, pane) || routes.shown.contains(pane)
            }
            other => other
                .pane_mut()
                .is_some_and(|pane| routes.attached(device, pane) || routes.shown.contains(pane)),
        };
        if !admitted {
            let lines = routes.unadmitted(device);
            drop(routes);
            log_unadmitted(device, "report", lines);
            return;
        }
        drop(routes);
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

    fn attach(pane: &str) -> TerminalControl {
        TerminalControl::Attach {
            pane: pane.into(),
            size: None,
            manual: false,
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
        router.control(attach("remote:mini:pane:w2:p3"));
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
        router.control(attach("remote:mini:pane:w2:p3"));
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

    /// A device's node is another machine's program: output for a pane the
    /// core never attached there, a report about a pane the core neither
    /// attached nor shows, a creation's discarded keys and a paste's outcome
    /// for a paste held elsewhere are not taken.
    #[test]
    fn a_devices_lines_the_core_never_asked_for_are_not_taken() {
        let (router, _local, hub, reports) = router();
        let mini = Arc::new(Recorder::default());
        let studio = Arc::new(Recorder::default());
        router.install_device("mini", Arc::clone(&mini) as Arc<dyn TerminalNode>);
        router.install_device("studio", Arc::clone(&studio) as Arc<dyn TerminalNode>);
        router.control(TerminalControl::Shown {
            panes: vec!["remote:mini:pane:w2:p4".into()],
        });
        router.control(attach("remote:mini:pane:w2:p3"));
        router.control(TerminalControl::AttachmentHold {
            pane: "remote:studio:pane:w1:p1".into(),
            intent: "paste-studio".into(),
        });
        router.control(TerminalControl::AttachmentHold {
            pane: "remote:mini:pane:w2:p3".into(),
            intent: "paste-mini".into(),
        });
        for pane in ["w2:p3", "w9:p9", "w2:p4"] {
            router.output("mini", pane, b"frame", true);
        }
        let error = |pane: &str| TerminalReport::Error {
            pane: pane.into(),
            kind: "terminal.write_failed".into(),
            message: "m".into(),
        };
        for report in [
            error("w2:p3"),
            error("w9:p9"),
            error("w2:p4"),
            TerminalReport::RequestDiscarded {
                request: "r1".into(),
                reason: "limit".into(),
            },
            TerminalReport::AttachmentDelivered {
                intent: "paste-studio".into(),
                written: true,
            },
            TerminalReport::AttachmentDelivered {
                intent: "paste-mini".into(),
                written: true,
            },
        ] {
            DeviceSink::report(&router, "mini", report);
        }
        assert_eq!(
            hub.output.lock().unwrap().as_slice(),
            [("remote:mini:pane:w2:p3".to_owned(), b"frame".to_vec())]
        );
        assert_eq!(
            reports.0.lock().unwrap().as_slice(),
            [
                error("remote:mini:pane:w2:p3"),
                error("remote:mini:pane:w2:p4"),
                TerminalReport::AttachmentDelivered {
                    intent: "paste-mini".into(),
                    written: true,
                },
            ]
        );
        // A forgotten pane's late output reaches no screen either.
        router.control(TerminalControl::Forget {
            pane: "remote:mini:pane:w2:p3".into(),
        });
        router.output("mini", "w2:p3", b"late", false);
        assert_eq!(hub.output.lock().unwrap().len(), 1);
        assert_eq!(
            hub.forgotten.lock().unwrap().as_slice(),
            ["remote:mini:pane:w2:p3"]
        );
    }

    /// A device's word that a key moved the keyboard counts only for the
    /// pane the last screen key went to; its input fact still counts.
    #[test]
    fn a_devices_focus_counts_only_for_the_pane_the_last_key_went_to() {
        let (router, _local, _hub, reports) = router();
        let mini = Arc::new(Recorder::default());
        router.install_device("mini", Arc::clone(&mini) as Arc<dyn TerminalNode>);
        router.control(attach("remote:mini:pane:w2:p3"));
        router.control(attach("remote:mini:pane:w2:p4"));
        let input = |pane: &str| TerminalReport::Input {
            pane: pane.into(),
            at_unix_ms: 7,
            submitted: false,
            focus: true,
        };
        router.key(
            KeyTarget::Pane("remote:mini:pane:w2:p3".into()),
            b"a".to_vec(),
            1,
        );
        DeviceSink::report(&router, "mini", input("w2:p3"));
        DeviceSink::report(&router, "mini", input("w2:p4"));
        router.key(KeyTarget::Pane("w1:p1".into()), b"b".to_vec(), 2);
        DeviceSink::report(&router, "mini", input("w2:p3"));
        let focus = reports
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|report| match report {
                TerminalReport::Input { pane, focus, .. } => (pane.clone(), *focus),
                other => panic!("unexpected {other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            focus,
            [
                ("remote:mini:pane:w2:p3".to_owned(), true),
                ("remote:mini:pane:w2:p4".to_owned(), false),
                ("remote:mini:pane:w2:p3".to_owned(), false),
            ]
        );
    }

    /// B5 on the failure path: a thousand keys into a device with no link
    /// are each refused, and the core hears it once a window.
    #[test]
    fn a_key_flood_into_a_device_without_a_link_is_reported_once_a_window() {
        let (router, _local, _hub, reports) = router();
        let started = Instant::now();
        for index in 0..1_000 {
            router.key(
                KeyTarget::Pane("remote:mini:pane:w2:p3".into()),
                b"k".to_vec(),
                index,
            );
        }
        let refused = reports.0.lock().unwrap().len() as u32;
        let windows = started.elapsed().as_secs() as u32 + 1;
        assert!(
            (1..=windows).contains(&refused),
            "{refused} reports in {windows} windows"
        );
    }
}
