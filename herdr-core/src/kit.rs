//! This Mac's install kit worker (PRD device-parity B1, B8, B10).
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

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::handle::ChangeNotifier;
use crate::runtime::{LocalKitJob, Runtime};
use crate::workspace::LOCAL_DEVICE_ID;

const PUMP_TICK: Duration = Duration::from_millis(250);

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
                    let diagnosis = matches!(job, LocalKitJob::Apply(_))
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

fn run(target: &hide_kit::KitTarget, job: &LocalKitJob) -> hide_kit::KitReport {
    let report = match job {
        LocalKitJob::Apply(scope) => hide_kit::apply(target, scope),
        LocalKitJob::Status => return hide_kit::status(target),
    };
    // One record per install pass, naming each part's outcome; the reasons
    // are the operator's diagnostic detail (engineering rule 10).
    crate::diagnostic!(json!({
        "component": "kit",
        "kind": "apply.completed",
        "device_id": LOCAL_DEVICE_ID,
        "scope": match job {
            LocalKitJob::Apply(hide_kit::Scope::Automatic) => "automatic",
            _ => "reinstall",
        },
        "components": report.components.iter().map(|part| json!({
            "id": part.id.code(),
            "state": part.state,
            "reason": part.reason,
        })).collect::<Vec<_>>(),
    }));
    report
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
