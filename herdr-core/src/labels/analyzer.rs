//! The one place label analyses run (PRD labels-in-hided D-05).
//!
//! One thread for the whole core takes jobs in arrival order and runs them
//! one at a time through a `hide-ai` router built from the operator's
//! current provider and model choice. Each Herdr server's worker hands in at
//! most one job at a time, so arrival order is a round robin between servers
//! and a busy server cannot starve another. The router's own one-in-flight
//! budget is therefore never exceeded, which would otherwise park a second
//! server's pane for the ten-minute provider wait.
//!
//! A changed choice rebuilds the router before the next job; the job already
//! running finishes on the router it started with (B17). `shutdown` cancels
//! the running request, which ends its provider child, and joins the thread,
//! so no request or provider process outlives the daemon (B22). With agent
//! summaries off (PRD overview-request-view D-11) a queued job is answered
//! as stopped without running, and `cancel_running` ends the one running.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use hide_ai::{AiRequest, AiResult, AiRouter, AiSettings, CancelToken, ProviderId};
use serde_json::json;

use super::analysis::{Analysis, AnalysisFailure};
use super::context_label;

pub(crate) type AnalysisResult = Result<(ProviderId, Analysis), AnalysisFailure>;

pub(crate) struct AnalysisJob {
    pub(crate) request: AiRequest,
    /// Called once on the analyzer thread with the outcome, including when
    /// the analyzer shuts down before running it.
    pub(crate) done: Box<dyn FnOnce(AnalysisResult) + Send>,
}

pub(crate) type SettingsSource = Box<dyn Fn() -> AiSettings + Send>;
pub(crate) type RouterFactory = Box<dyn Fn(&AiSettings) -> Arc<AiRouter> + Send>;

pub(crate) struct LabelAnalyzer {
    queue: Mutex<Option<Sender<AnalysisJob>>>,
    cancel: CancelToken,
    /// The running job's own token, cancelled on shutdown too.
    running: Arc<Mutex<Option<CancelToken>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl LabelAnalyzer {
    pub(crate) fn spawn(settings: SettingsSource) -> Result<Self, String> {
        Self::spawn_with(
            settings,
            Box::new(|settings| Arc::new(crate::ai::labels_router(settings))),
        )
    }

    pub(crate) fn spawn_with(
        settings: SettingsSource,
        router: RouterFactory,
    ) -> Result<Self, String> {
        let (sender, receiver) = channel();
        let cancel = CancelToken::new();
        let running = Arc::new(Mutex::new(None));
        let thread = Running {
            stopping: cancel.clone(),
            job: Arc::clone(&running),
        };
        let worker = std::thread::Builder::new()
            .name("herdr-core-labels-analyzer".to_owned())
            .spawn(move || run(receiver, settings, router, thread))
            .map_err(|error| format!("label analyzer could not be started: {error}"))?;
        Ok(Self {
            queue: Mutex::new(Some(sender)),
            cancel,
            running,
            worker: Mutex::new(Some(worker)),
        })
    }

    /// Queues a job. After shutdown the job is answered at once as stopped,
    /// so its pane never stays in flight.
    pub(crate) fn submit(&self, job: AnalysisJob) {
        let queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        let refused = match queue.as_ref() {
            Some(sender) => sender.send(job).err().map(|error| error.0),
            None => Some(job),
        };
        drop(queue);
        if let Some(job) = refused {
            (job.done)(Err(AnalysisFailure::Stopped));
        }
    }

    /// Ends the request running now, if any; it is answered as stopped.
    pub(crate) fn cancel_running(&self) {
        if let Some(job) = lock(&self.running).as_ref() {
            job.cancel();
        }
    }

    /// Cancels the running request, answers the queued ones and joins.
    pub(crate) fn shutdown(&self) {
        self.cancel.cancel();
        self.cancel_running();
        self.queue
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(worker) = worker
            && worker.join().is_err()
        {
            crate::diagnostic!(json!({"component": "labels", "kind": "analyzer.join_failed"}));
        }
    }
}

impl Drop for LabelAnalyzer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// What the analyzer thread is stopped by: the daemon stopping, or the one
/// job it runs being cancelled.
struct Running {
    stopping: CancelToken,
    job: Arc<Mutex<Option<CancelToken>>>,
}

fn run(
    receiver: Receiver<AnalysisJob>,
    settings: SettingsSource,
    build: RouterFactory,
    running: Running,
) {
    let mut router: Option<(AiSettings, Arc<AiRouter>)> = None;
    while let Ok(job) = receiver.recv() {
        let current = settings();
        if running.stopping.is_cancelled() || !current.agent_summary {
            (job.done)(Err(AnalysisFailure::Stopped));
            continue;
        }
        // The switch is not the router's: turning it does not rebuild one.
        let same_router = |built_for: &AiSettings| {
            built_for.provider == current.provider && built_for.models == current.models
        };
        let active = match router.as_ref() {
            Some((built_for, router)) if same_router(built_for) => Arc::clone(router),
            _ => {
                if router.is_some() {
                    crate::diagnostic!(json!({
                        "component": "labels",
                        "kind": "analyzer.settings_changed",
                        "provider": current.provider.to_string(),
                    }));
                }
                let built = build(&current);
                router = Some((current, Arc::clone(&built)));
                built
            }
        };
        let cancel = CancelToken::new();
        *lock(&running.job) = Some(cancel.clone());
        // A shutdown between the check above and the token's install.
        if running.stopping.is_cancelled() {
            cancel.cancel();
        }
        // The outcome must arrive whatever happens on this thread: a panic
        // that escaped would leave the pane in flight forever.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            analyze(&active, &job.request, &cancel)
        }))
        .unwrap_or_else(|_| {
            Err(AnalysisFailure::Worker(
                "analysis_worker_panicked".to_owned(),
            ))
        });
        lock(&running.job).take();
        (job.done)(result);
    }
}

fn analyze(router: &AiRouter, request: &AiRequest, cancel: &CancelToken) -> AnalysisResult {
    let AiResult { provider, value } = router.execute(request, cancel).map_err(|error| {
        if cancel.is_cancelled() {
            AnalysisFailure::Stopped
        } else {
            AnalysisFailure::Provider(error)
        }
    })?;
    let analysis = context_label::parse(value)
        .map_err(|error| AnalysisFailure::Invalid(format!("{error:#}")))?;
    Ok((provider, analysis))
}
