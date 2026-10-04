//! The install kit's workers (PRD device-parity B1, B8, B10, B13).
//!
//! One thread, owned by the core, runs `hide_kit` for this Mac: the launch
//! pass, the operator's Reinstall, and a re-read every few seconds while
//! Settings is on screen. It takes its job under the runtime lock and does
//! every file write, Herdr call and child process with the lock released.
//! It does not live on the session-sync coordinator, because that thread
//! exists only while this Mac's Herdr answers, and the kit's other parts
//! install whether or not Herdr is running.
//!
//! Dropping the pump raises the kit's stop flag, which ends a child the kit
//! is waiting on, and joins the thread (engineering rule 14).
//!
//! A device's kit runs on its helper (`hide_host::kit`); a worker per device
//! makes those calls while the device has kit work queued.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::handle::ChangeNotifier;
use crate::host_access::call_as;
use crate::runtime::{DeviceKitAnswer, DeviceKitCall, DeviceKitWork, KitJob, Runtime};
use crate::workspace::LOCAL_DEVICE_ID;

const PUMP_TICK: Duration = Duration::from_millis(250);

/// The longest a device's kit call may take: a plugin uninstall and
/// hcoord's daemon check are each bounded at twenty seconds on the device,
/// and Node is probed with five, so this outlasts a whole install.
const DEVICE_KIT_TIMEOUT: Duration = Duration::from_secs(180);

pub(crate) struct KitPump {
    stop: mpsc::Sender<()>,
    stop_flag: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

/// Where this Mac's kit runs, or why it cannot: only a daemon inside the app
/// bundle has parts that outlive it (B11).
pub(crate) fn local_target(
    kit_dir: Option<&str>,
    home: Option<PathBuf>,
    herdr_socket: PathBuf,
    stop: Arc<AtomicBool>,
) -> Result<hide_kit::KitTarget, String> {
    let kit_dir = kit_dir.ok_or_else(|| hide_kit::STANDALONE_REASON.to_owned())?;
    let home =
        home.ok_or_else(|| "HOME is not set, so Hide cannot tell where to install".to_owned())?;
    Ok(hide_kit::local_target(
        std::path::Path::new(kit_dir),
        &home,
        &herdr_socket,
        stop,
    ))
}

impl KitPump {
    pub(crate) fn spawn(
        runtime: Weak<Mutex<Runtime>>,
        notifier: ChangeNotifier,
        target: hide_kit::KitTarget,
    ) -> std::io::Result<Self> {
        let (stop, receiver) = mpsc::channel();
        let stop_flag = Arc::clone(&target.stop);
        let worker = thread::Builder::new()
            .name("kit-pump".into())
            .spawn(move || {
                while matches!(
                    receiver.recv_timeout(PUMP_TICK),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    let Some(core) = runtime.upgrade() else {
                        break;
                    };
                    let Ok(job) = core
                        .lock()
                        .map(|mut locked| locked.take_local_kit_job(Instant::now()))
                    else {
                        break;
                    };
                    drop(core);
                    let Some(job) = job else {
                        continue;
                    };
                    let report = run(&target, &job);
                    // The hook diagnosis reads the same files, so it is read
                    // again here: Memory's "update hooks" and the agent rows
                    // follow what the kit just wrote.
                    let diagnosis = matches!(job, KitJob::Apply(_))
                        .then(|| hide_agent_hooks::Diagnosis::read(&target.home));
                    let Some(core) = runtime.upgrade() else {
                        break;
                    };
                    let Ok(changed) = core.lock().map(|mut locked| {
                        let mut changed = locked.ingest_kit_report(LOCAL_DEVICE_ID, &report);
                        if let Some(diagnosis) = diagnosis {
                            changed |= locked.ingest_hook_diagnosis(diagnosis);
                        }
                        changed
                    }) else {
                        break;
                    };
                    drop(core);
                    if changed {
                        notifier.notify();
                    }
                }
            })?;
        Ok(Self {
            stop,
            stop_flag,
            worker: Some(worker),
        })
    }
}

fn run(target: &hide_kit::KitTarget, job: &KitJob) -> hide_kit::KitReport {
    match job {
        KitJob::Apply(scope) => {
            let report = hide_kit::apply(target, scope);
            completed(LOCAL_DEVICE_ID, scope, &report);
            report
        }
        KitJob::Status => hide_kit::status(target),
    }
}

/// One record per install pass on any machine, naming each part's outcome;
/// the reasons are the operator's diagnostic detail (engineering rule 10).
fn completed(device_id: &str, scope: &hide_kit::Scope, report: &hide_kit::KitReport) {
    crate::diagnostic!(json!({
        "component": "kit",
        "kind": "apply.completed",
        "device_id": device_id,
        "scope": if scope.is_automatic() { "automatic" } else { "operator" },
        "restore": scope.restore.iter().map(|id| id.code()).collect::<Vec<_>>(),
        "turn_off": scope.turn_off.iter().map(|id| id.code()).collect::<Vec<_>>(),
        "components": report.components.iter().map(|part| json!({
            "id": part.id.code(),
            "state": part.state,
            "reason": part.reason,
        })).collect::<Vec<_>>(),
    }));
}

/// Runs a device's queued kit calls on its helper, one at a time, until
/// nothing is queued or the helper is gone (`Runtime::take_device_kit_call`).
/// Each call is bounded by [`DEVICE_KIT_TIMEOUT`], and the worker ends with
/// the core, like the device's other helper workers.
pub(crate) fn spawn_device_worker(
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    device_id: String,
) -> std::io::Result<()> {
    thread::Builder::new()
        .name("herdr-core-device-kit".into())
        .spawn(move || {
            loop {
                let Some(core) = runtime.upgrade() else {
                    return;
                };
                let Ok(call) = core
                    .lock()
                    .map(|mut locked| locked.take_device_kit_call(&device_id))
                else {
                    return;
                };
                drop(core);
                let Some(call) = call else {
                    return;
                };
                let answer = call_device(&call);
                if let (
                    DeviceKitAnswer::Report(Ok(report)),
                    DeviceKitWork::Job(KitJob::Apply(scope)),
                ) = (&answer, &call.work)
                {
                    completed(&device_id, scope, report);
                }
                // A removed device's helper connection was kept open only
                // for this call.
                if call.work == DeviceKitWork::Remove {
                    call.channel.close("device removed");
                }
                let Some(core) = runtime.upgrade() else {
                    return;
                };
                let Ok(changed) = core
                    .lock()
                    .map(|mut locked| locked.ingest_device_kit_answer(&device_id, answer))
                else {
                    return;
                };
                drop(core);
                if changed {
                    notifier.notify();
                }
            }
        })
        .map(|_| ())
}

fn call_device(call: &DeviceKitCall) -> DeviceKitAnswer {
    use hide_host::protocol::KitAction;
    let action = match &call.work {
        DeviceKitWork::Job(KitJob::Apply(scope)) if scope.is_automatic() => KitAction::Apply,
        DeviceKitWork::Job(KitJob::Apply(scope)) => KitAction::Reinstall {
            components: scope.restore.iter().copied().collect(),
            turn_off: scope.turn_off.iter().copied().collect(),
        },
        DeviceKitWork::Job(KitJob::Status) => KitAction::Status,
        DeviceKitWork::Remove => KitAction::Remove,
    };
    let removing = action == KitAction::Remove;
    let kit = hide_host::protocol::Call::Kit {
        action,
        cli_dir: call.cli_dir.clone(),
        herdr_socket: call.herdr_socket.clone(),
    };
    if removing {
        DeviceKitAnswer::Removed(
            call_as(call.channel.as_ref(), kit, DEVICE_KIT_TIMEOUT)
                .map_err(|error| error.to_string()),
        )
    } else {
        DeviceKitAnswer::Report(
            call_as(call.channel.as_ref(), kit, DEVICE_KIT_TIMEOUT)
                .map_err(|error| error.to_string()),
        )
    }
}

impl Drop for KitPump {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
