//! The Software Factory host: the core owns one engine thread that runs every
//! Factory on this machine (PRD software-factory, Technical structure). The
//! engine never runs under `Mutex<Runtime>`; it takes the lock only to read
//! owned values or to ask the runtime for a delivery, a sleep or a wake.
//!
//! The engine opens its store the first time it is asked, or at start when a
//! store already exists, so a machine without a Factory pays nothing.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hide_factory::adapters::{
    Clock, EnvSignal, Environment, Failure, Judge, MemoryPressure, Notifier, Removal,
    WorkerRuntime, WorkerSpawn, WorkerStatus,
};
use hide_factory::exec::Machine;
use hide_factory::judgment::{Judgment, JudgmentAnswer, JudgmentOutcome};
use hide_factory::model::{Runtime as AgentRuntime, UnixMs, WorkerRef};
use hide_factory::project::{IssueBook, SharedProjects};
use hide_factory::role::Role;
use hide_factory::{Command, Engine, Inbound, Ports, Refusal};
use hide_node_link::factory::{FactoryCall, MemoryPressure as NodeMemoryPressure};
use hide_node_link::protocol::Call;
use serde_json::{Value, json};

use crate::delivery;
use crate::handle::ChangeNotifier;
use crate::runtime::Runtime;

pub mod screen;

use screen::{
    ActionAnswer, FactoryTaskSection, Publisher, ScreenRequest, ScreenSink, ScreenSource,
};

/// How often time and the outside world move the engine.
const TICK: Duration = Duration::from_secs(2);
const QUEUE_LIMIT: usize = 32;
/// The longest `add` waits for its review before answering pending (B10).
const REVIEW_WAIT: Duration = Duration::from_secs(90);
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(10);
/// A worker the snapshot does not show yet is still starting this long.
const START_GRACE_MS: u64 = 3 * 60_000;
/// The longest prompt argument; the rest is read with `hide factory show`.
const PROMPT_LIMIT: usize = 6 * 1024;
const JUDGMENT_DEADLINE: Duration = Duration::from_secs(180);

fn now_ms() -> UnixMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as UnixMs)
}

fn lock(runtime: &Weak<Mutex<Runtime>>) -> Option<Arc<Mutex<Runtime>>> {
    runtime.upgrade()
}

fn guard(runtime: &Arc<Mutex<Runtime>>) -> MutexGuard<'_, Runtime> {
    runtime.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Who is asking: the pane the command came from and the checkout it runs in.
pub struct FactoryCaller {
    pub pane: Option<String>,
    pub cwd: Option<String>,
    /// Another pane a pane-bound caller's hint named, which cannot be checked
    /// against its credential: it can only make the caller a worker, never an
    /// operator. A checkout-bound caller's hint is never read.
    pub claimed: Option<String>,
    /// The agents above the caller in the spawn lineage: a worker's child is
    /// a worker of the same Task (D-33).
    pub ancestors: Lineage,
}

/// The spawn lineage above a caller, nearest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lineage {
    pub agents: Vec<String>,
    /// The pane each of `agents` was registered on, where the ledger has it.
    pub panes: Vec<String>,
    /// The walk reached an agent with no parent. A cut walk (the ledger
    /// unreadable, a missing record, a loop, the step limit) cannot rule out
    /// a worker above the caller.
    pub complete: bool,
    /// A Factory's own agent is above the caller.
    pub factory_spawned: bool,
}

impl Lineage {
    /// No lineage: a caller with no agent record above it.
    pub fn none() -> Self {
        Self {
            agents: Vec::new(),
            panes: Vec::new(),
            complete: true,
            factory_spawned: false,
        }
    }
}

enum Request {
    /// A bounded read of an already-open engine. It never opens or ticks it.
    QuestionGuard {
        caller: QuestionCaller,
        runtime: Weak<Mutex<Runtime>>,
        deadline: Instant,
        reply: SyncSender<Result<bool, String>>,
    },
    Command {
        caller: FactoryCaller,
        command: Command,
        reply: SyncSender<Value>,
    },
    /// From a Factory screen, through a runtime event (PRD
    /// software-factory-ui); its answer comes back on the snapshot.
    Screen(ScreenRequest),
}

/// The runtime's way to the engine thread for the screens: a full queue is
/// refused at once, never waited on under the runtime lock.
#[derive(Clone)]
pub(crate) struct ScreenPort {
    requests: SyncSender<Request>,
}

impl ScreenPort {
    pub(crate) fn send(&self, request: ScreenRequest) -> Result<(), &'static str> {
        self.requests
            .try_send(Request::Screen(request))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "factory_busy",
                mpsc::TrySendError::Disconnected(_) => "factory_unavailable",
            })
    }
}

pub(crate) struct FactoryHost {
    requests: SyncSender<Request>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// A Factory command ready to run off the owner thread.
pub struct PreparedFactory {
    requests: SyncSender<Request>,
    caller: FactoryCaller,
    command: Command,
}

/// Owned native execution facts captured briefly under the Runtime lock.
pub(crate) struct QuestionCaller {
    pub actor: delivery::Actor,
    pub context: crate::workspace_control::Context,
    pub raw_pane: String,
    pub terminal_id: String,
    pub connector: Arc<dyn hide_herdr_client::ApiConnector>,
}

/// One readonly question decision, run outside both the owner and Runtime lock.
pub struct PreparedQuestionGuard {
    requests: SyncSender<Request>,
    caller: QuestionCaller,
    runtime: Weak<Mutex<Runtime>>,
    deadline: Instant,
}

impl PreparedQuestionGuard {
    pub fn run(self) -> Result<bool, String> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or("factory_guard_expired")?;
        let fresh = hide_herdr_client::request_small_response_until(
            self.caller.connector.as_ref(),
            "agent.get",
            crate::wire::agent_target_params(&self.caller.raw_pane)?,
            self.deadline,
        )
        .map_err(|_| "factory_guard_native_unavailable")?;
        let fresh =
            crate::wire::delivery_agent(fresh).map_err(|_| "factory_guard_native_unavailable")?;
        if fresh.pane_id != self.caller.raw_pane
            || fresh.terminal_id != self.caller.terminal_id
            || fresh.name != self.caller.actor.name
            || fresh.kind.as_deref() != Some(self.caller.actor.kind.as_str())
            || fresh.session.is_none()
            || fresh.session != self.caller.actor.session
        {
            return Err("factory_guard_native_changed".into());
        }
        let (reply, answer) = mpsc::sync_channel(1);
        self.requests
            .try_send(Request::QuestionGuard {
                caller: self.caller,
                runtime: self.runtime,
                deadline: self.deadline,
                reply,
            })
            .map_err(|_| "factory_guard_busy")?;
        let left = self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or("factory_guard_expired")?;
        answer
            .recv_timeout(left)
            .map_err(|_| "factory_guard_expired")?
    }
}

impl PreparedFactory {
    pub fn run(self, timeout: Duration) -> Result<Value, String> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.requests
            .try_send(Request::Command {
                caller: self.caller,
                command: self.command,
                reply,
            })
            .map_err(|_| "factory_busy".to_owned())?;
        answer
            .recv_timeout(timeout)
            .map_err(|_| "factory_timeout".to_owned())
    }
}

impl FactoryHost {
    pub(crate) fn start(
        state_dir: &Path,
        home: Option<PathBuf>,
        runtime: Weak<Mutex<Runtime>>,
        notifier: ChangeNotifier,
    ) -> Result<Self, String> {
        let (requests, receiver) = mpsc::sync_channel(QUEUE_LIMIT);
        let stop = Arc::new(AtomicBool::new(false));
        let paths = Paths {
            store: hide_kit::layout::factory_store(state_dir),
            files: hide_kit::layout::factory_files(state_dir),
        };
        let thread_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("herdr-core-factory".into())
            .spawn(move || run(paths, home, runtime, notifier, receiver, thread_stop))
            .map_err(|error| format!("factory engine thread could not start: {error}"))?;
        Ok(Self {
            requests,
            stop,
            thread: Some(thread),
        })
    }

    pub(crate) fn screen_port(&self) -> ScreenPort {
        ScreenPort {
            requests: self.requests.clone(),
        }
    }

    pub(crate) fn prepare(&self, caller: FactoryCaller, command: Command) -> PreparedFactory {
        PreparedFactory {
            requests: self.requests.clone(),
            caller,
            command,
        }
    }

    pub(crate) fn prepare_question_guard(
        &self,
        caller: QuestionCaller,
        runtime: Weak<Mutex<Runtime>>,
        deadline: Instant,
    ) -> PreparedQuestionGuard {
        PreparedQuestionGuard {
            requests: self.requests.clone(),
            caller,
            runtime,
            deadline,
        }
    }

    pub(crate) fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            crate::diagnostic!(json!({"component":"factory","kind":"engine.join_failed"}));
        }
    }
}

impl Drop for FactoryHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Paths {
    store: PathBuf,
    files: PathBuf,
}

/// A review an `add` waits for (B10).
struct Waiter {
    factory: String,
    task: String,
    reply: SyncSender<Value>,
    until: Instant,
}

/// Shared between the engine thread and its worker port.
#[derive(Default)]
struct WorkerState {
    /// Panes whose sleep was asked while the agent was still in its turn.
    pending_sleep: BTreeSet<String>,
    /// Woken workers whose agent has not come back yet, with the letters it
    /// gets once it has: a letter needs the agent's session to address.
    waking: BTreeMap<String, Woken>,
    /// Worker starts, carried out on the starter thread so a start that
    /// waits for Herdr never holds a command or a worker's report (B23).
    starts: Starts,
}

/// Starts by `factory/task`: queued or running, finished and not yet asked
/// for, and ones the engine gave up on while they ran.
#[derive(Default)]
struct Starts {
    queue: Option<SyncSender<WorkerSpawn>>,
    in_flight: BTreeSet<String>,
    done: BTreeMap<String, Finished>,
    abandoned: BTreeSet<String>,
}

/// A start the engine has not asked for yet.
struct Finished {
    /// A new worker, whose worktree the start made.
    fresh: bool,
    result: Result<WorkerRef, Failure>,
}

/// Starts waiting for the starter thread; one past it is asked again later.
const START_QUEUE_LIMIT: usize = 16;

fn start_key(factory: &str, task: &str) -> String {
    format!("{factory}/{task}")
}

/// Runs queued worker starts one at a time until the host drops the queue.
/// Once the host stops, a start still queued is dropped rather than run.
/// A start the engine gave up on while it ran is released: its worker
/// stopped and, for a new worker, the worktree it made removed.
fn run_starts(
    jobs: Receiver<WorkerSpawn>,
    state: Arc<Mutex<WorkerState>>,
    stop: Arc<AtomicBool>,
    start: impl Fn(&WorkerSpawn) -> Result<WorkerRef, Failure>,
    release: impl Fn(&WorkerSpawn, &WorkerRef),
) {
    for job in jobs {
        let key = start_key(&job.factory, &job.task);
        if stop.load(Ordering::Acquire) {
            if let Ok(mut state) = state.lock() {
                state.starts.in_flight.remove(&key);
            }
            continue;
        }
        let result = start(&job);
        let abandoned = {
            let Ok(mut state) = state.lock() else { return };
            state.starts.in_flight.remove(&key);
            if state.starts.abandoned.remove(&key) {
                true
            } else {
                state.starts.done.insert(
                    key,
                    Finished {
                        fresh: job.resume.is_none(),
                        result: result.clone(),
                    },
                );
                false
            }
        };
        if abandoned && let Ok(worker) = result {
            release(&job, &worker);
        }
    }
}

/// A finished start, `start_pending` while one is queued or running, or
/// `None` when this Task has no start yet.
fn poll_start(state: &Mutex<WorkerState>, key: &str) -> Option<Result<WorkerRef, Failure>> {
    let Ok(mut state) = state.lock() else {
        return Some(Err(Failure::task("worker.spawn", "state_unavailable")));
    };
    if let Some(finished) = state.starts.done.remove(key) {
        return Some(finished.result);
    }
    state
        .starts
        .in_flight
        .contains(key)
        .then(|| Err(Failure::start_pending("worker.spawn")))
}

/// Hands a start to the starter thread; a full queue is asked again on a
/// later tick, never waited on.
fn queue_start(state: &Mutex<WorkerState>, request: &WorkerSpawn) -> Failure {
    let key = start_key(&request.factory, &request.task);
    let Ok(mut state) = state.lock() else {
        return Failure::task("worker.spawn", "state_unavailable");
    };
    let Some(queue) = state.starts.queue.clone() else {
        return Failure::starting("worker.spawn", "starter_stopped");
    };
    match queue.try_send(request.clone()) {
        Ok(()) => {
            state.starts.in_flight.insert(key);
            Failure::start_pending("worker.spawn")
        }
        Err(mpsc::TrySendError::Full(_)) => Failure::start_pending("worker.spawn"),
        Err(mpsc::TrySendError::Disconnected(_)) => {
            Failure::starting("worker.spawn", "starter_stopped")
        }
    }
}

struct Woken {
    worker: WorkerRef,
    letters: Vec<(String, String)>,
    since: Instant,
}

/// Letters held for one woken worker, and how long a wake may take before
/// they go to the diagnostic log instead (the engine's no-report rule then
/// stops the Task).
const WOKEN_LETTER_LIMIT: usize = 16;
const WAKE_LIMIT: Duration = Duration::from_secs(10 * 60);

fn run(
    paths: Paths,
    home: Option<PathBuf>,
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
    requests: Receiver<Request>,
    stop: Arc<AtomicBool>,
) {
    let mut publisher = Publisher::default();
    let mut sink = RuntimeSink {
        runtime: runtime.clone(),
        notifier,
    };
    let workers = Arc::new(Mutex::new(WorkerState::default()));
    let (start_queue, start_jobs) = mpsc::sync_channel(START_QUEUE_LIMIT);
    if let Ok(mut state) = workers.lock() {
        state.starts.queue = Some(start_queue);
    }
    let starter = {
        let runtime = runtime.clone();
        let state = Arc::clone(&workers);
        let stop = Arc::clone(&stop);
        thread::Builder::new()
            .name("factory-starts".into())
            .spawn(move || {
                let start = |job: &WorkerSpawn| start_worker(&runtime, job);
                let release = |job: &WorkerSpawn, worker: &WorkerRef| {
                    let mut port = CoreWorkers {
                        runtime: runtime.clone(),
                        state: Arc::clone(&state),
                    };
                    port.release_start(&job.task, job.resume.is_none(), worker);
                };
                run_starts(start_jobs, Arc::clone(&state), stop, start, release);
            })
    };
    if let Err(error) = &starter {
        crate::diagnostic!(
            json!({"component":"factory","kind":"starts.spawn_failed","error":error.to_string()})
        );
    }
    let runner_stop = Arc::clone(&stop);
    let mut engine: Option<Engine> = None;
    let mut judge: Option<JudgeThread> = None;
    let open = |judge: &mut Option<JudgeThread>| -> Option<Engine> {
        // The Factory decides here; its machine work is the core's own
        // node's (PRD core-host-node D-01).
        let machine = Machine::new(guard(&lock(&runtime)?).own_node(), Arc::clone(&runner_stop));
        let started = JudgeThread::start(runtime.clone(), home.clone());
        let port = started.port();
        *judge = Some(started);
        let projects = SharedProjects::new(
            machine.clone(),
            Box::new(CoreIssues {
                runtime: runtime.clone(),
            }),
            paths.files.join("logs"),
        );
        let ports = Ports {
            clock: Box::new(SystemClock::new(machine.clone())),
            source: Box::new(projects.clone()),
            verifier: Box::new(projects.clone()),
            merge: Box::new(projects),
            workers: Box::new(CoreWorkers {
                runtime: runtime.clone(),
                state: Arc::clone(&workers),
            }),
            judge: Box::new(port),
            environment: Box::new(MachineEnvironment(machine.clone())),
            notifier: Box::new(CoreNotifier {
                runtime: runtime.clone(),
            }),
        };
        match Engine::open(&paths.store, &paths.files, ports) {
            Ok(mut engine) => {
                match machine.call::<Option<String>>("hide_program", FactoryCall::HideProgram) {
                    Ok(Some(program)) => engine.set_hide_program(program),
                    Ok(None) => {}
                    Err(failure) => crate::diagnostic!(json!({
                        "component": "factory",
                        "kind": "hide_program.unread",
                        "error": failure.detail,
                    })),
                }
                Some(engine)
            }
            Err(error) => {
                crate::diagnostic!(
                    json!({"component":"factory","kind":"store.open_failed","error":error.0})
                );
                None
            }
        }
    };
    if paths.store.exists() {
        engine = open(&mut judge);
        publish_recipients(engine.as_ref(), &runtime);
    }
    run_requests(
        &mut engine,
        &mut publisher,
        &mut sink,
        &workers,
        &requests,
        &stop,
        || open(&mut judge),
        Instant::now,
    );
    drop(engine);
    drop(judge);
    // The stop flag is set: closing the queue ends the starter after the
    // start it is running, and the starts still queued are dropped.
    if let Ok(mut state) = workers.lock() {
        state.starts.queue = None;
    }
    if let Ok(starter) = starter
        && starter.join().is_err()
    {
        crate::diagnostic!(json!({"component":"factory","kind":"starts.join_failed"}));
    }
}

// Keep the existing owner objects explicit; the clock controls only this
// loop's scheduler, independently of the engine's wall clock and query expiry.
#[allow(clippy::too_many_arguments)]
fn run_requests(
    engine: &mut Option<Engine>,
    publisher: &mut Publisher,
    sink: &mut RuntimeSink,
    workers: &Arc<Mutex<WorkerState>>,
    requests: &Receiver<Request>,
    stop: &AtomicBool,
    mut open: impl FnMut() -> Option<Engine>,
    mut clock: impl FnMut() -> Instant,
) {
    let runtime = sink.runtime.clone();
    let mut waiters: Vec<Waiter> = Vec::new();
    let mut last_tick = clock();
    while !stop.load(Ordering::Acquire) {
        // A request cannot renew the scheduled tick's waiting time.
        let remaining = request_wait(engine.as_ref(), last_tick, clock());
        let read_only = match requests.recv_timeout(remaining) {
            Ok(Request::QuestionGuard {
                caller,
                runtime,
                deadline,
                reply,
            }) => {
                let answer = question_guard(engine.as_ref(), &runtime, &caller, deadline);
                let _ = reply.try_send(answer);
                true
            }
            Ok(Request::Command {
                caller,
                command,
                reply,
            }) => {
                if engine.is_none() {
                    *engine = open();
                }
                let Some(engine) = engine.as_mut() else {
                    let _ = reply.send(
                        Refusal::new("factory_unavailable", "See the diagnostic log").to_json(),
                    );
                    continue;
                };
                let added = matches!(command, Command::Add { .. });
                let answer = handle(engine, &runtime, &caller, command);
                // A worker's `decide` or a comment changes the open page and
                // may leave the summary as it was.
                publisher.touched();
                publish_recipients(Some(engine), &runtime);
                if added
                    && answer["result"] == "pending"
                    && let (Some(factory), Some(task)) =
                        (owner_of(engine, &answer), answer["task"]["id"].as_str())
                {
                    waiters.push(Waiter {
                        factory,
                        task: task.to_owned(),
                        reply,
                        until: clock() + REVIEW_WAIT,
                    });
                    continue;
                }
                let _ = reply.send(answer);
                false
            }
            Ok(Request::Screen(request)) => {
                if engine.is_none() && !matches!(request, ScreenRequest::CloseTask) {
                    *engine = open();
                }
                screen_request(engine.as_mut(), publisher, sink, request);
                publish_recipients(engine.as_ref(), &runtime);
                false
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let now = clock();
        let tick_due = now.saturating_duration_since(last_tick) >= TICK;
        // Guards never open, mutate or publish on their own. An already-due
        // tick still owns normal maintenance, even while reads keep arriving.
        if read_only && (engine.is_none() || !tick_due) {
            continue;
        }
        let Some(engine) = engine.as_mut() else {
            publisher.publish(None, sink);
            continue;
        };
        if tick_due {
            last_tick = now;
            pump_letters(engine, &runtime);
            engine.tick();
            // A running attempt's log tail grows on the open page between
            // summary changes; an unchanged page is still dropped.
            publisher.touched();
            settle_sleeps(workers, &runtime);
            let mut port = CoreWorkers {
                runtime: runtime.clone(),
                state: Arc::clone(workers),
            };
            port.deliver_woken();
            publish_recipients(Some(engine), &runtime);
        }
        for failure in engine.take_store_failures() {
            crate::diagnostic!(json!({
                "component": "factory",
                "kind": "store.write_failed",
                "factory_id": failure.factory,
                "task_id": failure.task,
                "stage": failure.stage,
                "error": failure.error,
            }));
        }
        let now = clock();
        waiters.retain(|waiter| {
            let settled = engine.review_settled(&waiter.factory, &waiter.task);
            if settled || now >= waiter.until {
                let _ = waiter
                    .reply
                    .send(engine.add_answer(&waiter.factory, &waiter.task));
                return false;
            }
            true
        });
        // Off the runtime lock; an unchanged summary hands nothing over.
        publisher.publish(Some(&*engine as &dyn ScreenSource), sink);
    }
}

fn request_wait(engine: Option<&Engine>, last_tick: Instant, now: Instant) -> Duration {
    // There is no scheduled engine work before opening. Keep the ordinary
    // idle wait instead of spinning on an overdue tick that cannot run.
    if engine.is_none() {
        TICK
    } else {
        TICK.saturating_sub(now.saturating_duration_since(last_tick))
    }
}

fn question_guard(
    engine: Option<&Engine>,
    runtime: &Weak<Mutex<Runtime>>,
    caller: &QuestionCaller,
    deadline: Instant,
) -> Result<bool, String> {
    if Instant::now() >= deadline {
        return Err("factory_guard_expired".into());
    }
    let engine = engine.ok_or("factory_guard_unstarted")?;
    let Some(worker) = engine.question_worker(&caller.actor.pane_id) else {
        return Ok(false);
    };
    let runtime = runtime.upgrade().ok_or("factory_guard_unavailable")?;
    let runtime = runtime.try_lock().map_err(|_| "factory_guard_busy")?;
    runtime.factory_question_current(caller, worker)?;
    if Instant::now() >= deadline {
        return Err("factory_guard_expired".into());
    }
    Ok(true)
}

/// Runs one screen request with the operator role the screen holds; every
/// answer it gives is recorded as relayed by `screen`.
fn screen_request(
    engine: Option<&mut Engine>,
    publisher: &mut Publisher,
    sink: &mut RuntimeSink,
    request: ScreenRequest,
) {
    let operator = Role::Operator {
        pane: screen::SCREEN_OPERATOR.into(),
    };
    match request {
        ScreenRequest::Action {
            request_id,
            command,
        } => {
            let answer = match engine {
                None => Refusal::new("factory_unavailable", "See the diagnostic log").to_json(),
                Some(_) if !screen::screen_may_send(&command) => {
                    Refusal::new("factory_screen_verb", "Use hide factory for this command")
                        .to_json()
                }
                Some(engine) => engine.command(&operator, command),
            };
            publisher.touched();
            sink.answered(ActionAnswer { request_id, answer });
        }
        ScreenRequest::OpenTask { factory, task } => {
            // A person opening a finished Task has seen it (D-30): the same
            // `show` the CLI runs, as the operator.
            if let Some(engine) = engine {
                let _ = engine.command(
                    &operator,
                    Command::Show {
                        task: format!("{factory}/{task}"),
                    },
                );
            }
            publisher.open(factory, task);
        }
        ScreenRequest::CloseTask => publisher.close(),
    }
}

/// Hands the screens' values to the runtime under one short lock each and
/// announces them.
struct RuntimeSink {
    runtime: Weak<Mutex<Runtime>>,
    notifier: ChangeNotifier,
}

impl ScreenSink for RuntimeSink {
    fn publish(
        &mut self,
        summary: Option<Arc<hide_factory::FactorySummary>>,
        task: Option<Option<FactoryTaskSection>>,
    ) {
        let Some(runtime) = lock(&self.runtime) else {
            return;
        };
        guard(&runtime).set_factory_screen(summary, task);
        drop(runtime);
        self.notifier.notify();
    }

    fn answered(&mut self, answer: ActionAnswer) {
        let Some(runtime) = lock(&self.runtime) else {
            return;
        };
        guard(&runtime).factory_answered(answer);
        drop(runtime);
        self.notifier.notify();
    }
}

fn owner_of(engine: &Engine, answer: &Value) -> Option<String> {
    let task = answer["task"]["id"].as_str()?;
    engine
        .factories()
        .find(|factory| engine.task(&factory.id, task).is_some())
        .map(|factory| factory.id.clone())
}

/// Open Factories become delivery recipients; closed ones stop receiving.
fn publish_recipients(engine: Option<&Engine>, runtime: &Weak<Mutex<Runtime>>) {
    let ids: BTreeMap<String, u64> = engine
        .into_iter()
        .flat_map(Engine::factories)
        .filter(|factory| !factory.closed)
        .map(|factory| (factory.id.clone(), factory.config.stall_ms))
        .collect();
    if let Some(runtime) = lock(runtime) {
        guard(&runtime).set_factory_recipients(ids);
    }
}

/// The delivery intent of a worker's report. A retry of the same report in
/// the same Task state is one letter; the same words after the Task moved
/// (a `done` again once verification sent it back) are a new report.
fn report_intent(pane: &str, epoch: u64, body: &str) -> String {
    format!(
        "factory-{}",
        &hide_factory::store::sha256_hex(format!("{pane}\n{epoch}\n{body}").as_bytes())[..24]
    )
}

/// Runs one command with the caller's role (D-33). A worker's report travels
/// as a ledger letter from its own pane, so a harness that only speaks the
/// letter protocol lands on the same path (B25).
fn handle(
    engine: &mut Engine,
    runtime: &Weak<Mutex<Runtime>>,
    caller: &FactoryCaller,
    command: Command,
) -> Value {
    let facts = hide_factory::engine::Caller {
        pane: caller.pane.as_deref(),
        cwd: caller.cwd.as_deref(),
        claimed: caller.claimed.as_deref(),
        ancestor_agents: &caller.ancestors.agents,
        ancestor_panes: &caller.ancestors.panes,
        lineage_complete: caller.ancestors.complete,
        factory_spawned: caller.ancestors.factory_spawned,
    };
    let role = match engine.caller_role(&facts, &command) {
        Ok(role) => role,
        Err(refusal) => return refusal.to_json(),
    };
    // A pending review answers the pane that added the Task (B11); that is
    // the caller, never a pane the request names.
    // A command that names no project means the Factory of the caller's
    // checkout, when it has one; `status` without one still shows them all.
    let here = || {
        caller
            .cwd
            .as_deref()
            .and_then(|cwd| engine.factory_for_project(cwd))
            .map(|factory| factory.project.clone())
    };
    let command = match command {
        Command::Add {
            project,
            task,
            issue,
            card,
            producer_pane: _,
        } => Command::Add {
            project: project.or_else(here),
            task,
            issue,
            card,
            producer_pane: caller.pane.clone(),
        },
        Command::Config { project, set } => Command::Config {
            project: project.or_else(here),
            set,
        },
        Command::Close { project } => Command::Close {
            project: project.or_else(here),
        },
        Command::Check {
            project,
            at,
            instruction,
        } => Command::Check {
            project: project.or_else(here),
            at,
            instruction,
        },
        other => other,
    };
    let kind = match &command {
        Command::Ask { .. } | Command::Propose { .. } => Some("request"),
        Command::Block { .. } => Some("block"),
        Command::Done { .. } => Some("report"),
        _ => None,
    };
    let (Role::Worker { factory, task }, Some(kind), Some(pane)) = (&role, kind, &caller.pane)
    else {
        return engine.command(&role, command);
    };
    let body = json!({"factory": command}).to_string();
    let epoch = engine
        .task(factory, task)
        .map_or(0, |task| task.state_since);
    let intent = report_intent(pane, epoch, &body);
    let letter = (|| {
        let runtime = lock(runtime).ok_or("delivery_unavailable")?;
        let prepared = guard(&runtime).factory_worker_letter(
            pane,
            factory,
            delivery::Command::Send {
                target: format!("{}{factory}", delivery::FACTORY_PREFIX),
                intent,
                body: body.clone(),
                kind: kind.into(),
            },
        )?;
        prepared.run(DELIVERY_TIMEOUT)
    })();
    let letter = match letter {
        Ok(letter) => letter,
        Err(reason) => {
            return Refusal::new(&reason, "Retry; the report is applied once").to_json();
        }
    };
    let Some(id) = letter["id"].as_str() else {
        return Refusal::new("letter_unavailable", "Retry").to_json();
    };
    let answer = engine.letter(Inbound {
        id: id.to_owned(),
        factory: factory.clone(),
        sender_pane: pane.clone(),
        kind: kind.to_owned(),
        body,
    });
    close_letter(runtime, factory, id, kind, &answer);
    answer
}

/// Takes a letter in and answers a request at once; a later answer from a
/// person reaches the worker as its own letter.
fn close_letter(
    runtime: &Weak<Mutex<Runtime>>,
    factory: &str,
    id: &str,
    kind: &str,
    answer: &Value,
) {
    let mut commands = vec![delivery::Command::Confirm {
        ids: vec![id.to_owned()],
    }];
    if matches!(kind, "request" | "block") {
        commands.push(delivery::Command::Reply {
            id: id.to_owned(),
            intent: format!("{id}-answer"),
            body: answer.to_string(),
        });
    }
    for command in commands {
        let result = (|| {
            let runtime = lock(runtime).ok_or("delivery_unavailable")?;
            let prepared = guard(&runtime).factory_prepare(factory, None, command)?;
            prepared.run(DELIVERY_TIMEOUT)
        })();
        if let Err(reason) = result {
            crate::diagnostic!(
                json!({"component":"factory","kind":"letter.close_failed","factory":factory,"letter":id,"reason":reason})
            );
        }
    }
}

/// Letters a harness or a watch sent to a Factory (B25, B24).
fn pump_letters(engine: &mut Engine, runtime: &Weak<Mutex<Runtime>>) {
    let Some(arc) = lock(runtime) else { return };
    let ledger = match guard(&arc).delivery_state() {
        Ok(ledger) => ledger,
        Err(_) => return,
    };
    drop(arc);
    let pending: Vec<(String, Inbound)> = ledger
        .letters
        .iter()
        .filter(|letter| letter.recipient.code_owned() && !letter.intake_confirmed())
        .filter(|letter| !engine.letter_seen(&letter.id))
        .filter_map(|letter| {
            let factory = letter
                .recipient
                .pane_id
                .strip_prefix(delivery::FACTORY_PREFIX)?
                .to_owned();
            let sender_pane = match &letter.watch_warning {
                Some(warning) => warning.target.pane_id.clone(),
                None => letter.sender.pane_id.clone(),
            };
            Some((
                letter.kind.clone(),
                Inbound {
                    id: letter.id.clone(),
                    factory,
                    sender_pane,
                    kind: letter.kind.clone(),
                    body: letter.body.clone(),
                },
            ))
        })
        .take(16)
        .collect();
    for (kind, letter) in pending {
        let factory = letter.factory.clone();
        let id = letter.id.clone();
        let answer = engine.letter(letter);
        close_letter(runtime, &factory, &id, &kind, &answer);
    }
}

fn settle_sleeps(state: &Arc<Mutex<WorkerState>>, runtime: &Weak<Mutex<Runtime>>) {
    let panes: Vec<String> = state
        .lock()
        .map(|state| state.pending_sleep.iter().cloned().collect())
        .unwrap_or_default();
    if panes.is_empty() {
        return;
    }
    let Some(runtime) = lock(runtime) else { return };
    let mut done = Vec::new();
    {
        let mut runtime = guard(&runtime);
        for pane in &panes {
            match runtime.factory_sleep(pane) {
                Ok(true) => done.push(pane.clone()),
                Ok(false) => {}
                Err(reason) => {
                    crate::diagnostic!(
                        json!({"component":"factory","kind":"worker.sleep_refused","pane_id":pane,"reason":reason})
                    );
                    done.push(pane.clone());
                }
            }
        }
    }
    if let Ok(mut state) = state.lock() {
        for pane in done {
            state.pending_sleep.remove(&pane);
        }
    }
}

/// The time, and the local day's offset as the core's own node reads it.
/// A read the node cannot answer keeps the last offset it gave, so a busy
/// moment does not move the done-today day; before any answer it is UTC.
struct SystemClock {
    machine: Machine,
    /// The last offset the node answered; [`i64::MIN`] before the first.
    last: AtomicI64,
    /// Whether the last read failed, so a run of failures is said once.
    failing: AtomicBool,
}

impl SystemClock {
    fn new(machine: Machine) -> Self {
        Self {
            machine,
            last: AtomicI64::new(i64::MIN),
            failing: AtomicBool::new(false),
        }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> UnixMs {
        now_ms()
    }

    fn utc_offset_ms(&self) -> i64 {
        match self.machine.read::<i64>(FactoryCall::UtcOffset) {
            Ok(offset) => {
                self.last.store(offset, Ordering::Relaxed);
                self.failing.store(false, Ordering::Relaxed);
                offset
            }
            Err(error) => {
                let last = self.last.load(Ordering::Relaxed);
                if !self.failing.swap(true, Ordering::Relaxed) {
                    crate::diagnostic!(serde_json::json!({
                        "component": "factory",
                        "kind": "clock.offset_unavailable",
                        "error": error.to_string(),
                        "kept_last": last != i64::MIN,
                    }));
                }
                if last == i64::MIN { 0 } else { last }
            }
        }
    }
}

/// Starts, messages, sleeps and wakes workers through coordination, the
/// delivery ledger and agent sleep (D-14). Nothing here holds the runtime
/// lock across Herdr or ledger work.
struct CoreWorkers {
    runtime: Weak<Mutex<Runtime>>,
    state: Arc<Mutex<WorkerState>>,
}

impl CoreWorkers {
    /// A start no Task claims: its worker stops and, when the start made a
    /// new worktree, that worktree and its branch go too.
    fn release_start(&mut self, task: &str, fresh: bool, worker: &WorkerRef) {
        let mut result = self.stop(worker);
        if fresh && result.is_ok() {
            result = self.remove_worktree(worker, Removal::Discarded);
        }
        if let Err(failure) = result {
            crate::diagnostic!(
                json!({"component":"factory","kind":"worker.abandon_failed","task":task,"stage":failure.stage})
            );
        }
    }
    /// Hands each woken worker its letters once its agent is back.
    fn deliver_woken(&mut self) {
        let panes: Vec<String> = self
            .state
            .lock()
            .map(|state| state.waking.keys().cloned().collect())
            .unwrap_or_default();
        for pane in panes {
            let Ok(runtime) = self.runtime() else { return };
            let probe = guard(&runtime).factory_worker_probe(&pane);
            drop(runtime);
            let Some(woken) = self
                .state
                .lock()
                .ok()
                .and_then(|mut state| state.waking.remove(&pane))
            else {
                continue;
            };
            if !probe.present || probe.asleep {
                if woken.since.elapsed() >= WAKE_LIMIT {
                    crate::diagnostic!(
                        json!({"component":"factory","kind":"worker.wake_timed_out","pane_id":pane,"letters":woken.letters.len()})
                    );
                } else if let Ok(mut state) = self.state.lock() {
                    state.waking.insert(pane, woken);
                }
                continue;
            }
            let mut left = Vec::new();
            for (intent, body) in woken.letters {
                if !left.is_empty() {
                    left.push((intent, body));
                    continue;
                }
                if let Err(failure) = self.message(&woken.worker, &intent, None, &body) {
                    // The agent is back but its session is not observed yet.
                    if woken.since.elapsed() >= WAKE_LIMIT {
                        crate::diagnostic!(
                            json!({"component":"factory","kind":"worker.wake_letter_failed","pane_id":pane,"reason":failure.detail})
                        );
                        continue;
                    }
                    left.push((intent, body));
                }
            }
            if !left.is_empty()
                && let Ok(mut state) = self.state.lock()
            {
                state.waking.insert(
                    pane,
                    Woken {
                        letters: left,
                        ..woken
                    },
                );
            }
        }
    }

    fn runtime(&self) -> Result<Arc<Mutex<Runtime>>, Failure> {
        lock(&self.runtime)
            .ok_or_else(|| Failure::environment("worker", EnvSignal::HerdrSocket, "core stopped"))
    }

    fn deliver(
        &self,
        factory: &str,
        target: Option<&str>,
        command: delivery::Command,
    ) -> Result<Value, Failure> {
        let runtime = self.runtime()?;
        let prepared = guard(&runtime)
            .factory_prepare(factory, target, command)
            .map_err(|reason| Failure::task("delivery", reason))?;
        drop(runtime);
        prepared
            .run(DELIVERY_TIMEOUT)
            .map_err(|reason| Failure::task("delivery", reason))
    }

    /// The watch that stops a quiet worker after 30 minutes (B24).
    fn watch(&self, worker: &WorkerRef) {
        let Some(pane) = &worker.pane else { return };
        if let Err(failure) = self.deliver(
            &worker.factory,
            Some(pane),
            delivery::Command::WatchStart {
                target: pane.clone(),
                observer: None,
                actor: None,
            },
        ) {
            crate::diagnostic!(
                json!({"component":"factory","kind":"worker.watch_failed","pane_id":pane,"reason":failure.detail})
            );
        }
    }
}

impl WorkerRuntime for CoreWorkers {
    /// Hands a finished start back, or queues one for the starter thread
    /// and answers that it is pending.
    fn spawn(&mut self, request: &WorkerSpawn) -> Result<WorkerRef, Failure> {
        let key = start_key(&request.factory, &request.task);
        if let Some(answer) = poll_start(&self.state, &key) {
            return answer;
        }
        let runtime = self.runtime()?;
        if request.runtime == hide_factory::model::Runtime::Codex
            && !guard(&runtime).factory_kit_read()
        {
            return Err(Failure::starting("worker.spawn", "kit_not_read"));
        }
        drop(runtime);
        Err(queue_start(&self.state, request))
    }

    fn abandon_start(&mut self, factory: &str, task: &str) -> bool {
        let key = start_key(factory, task);
        let (in_flight, finished) = match self.state.lock() {
            Ok(mut state) => {
                let in_flight = state.starts.in_flight.contains(&key);
                if in_flight {
                    state.starts.abandoned.insert(key.clone());
                }
                (in_flight, state.starts.done.remove(&key))
            }
            Err(_) => (false, None),
        };
        let taken = in_flight || finished.is_some();
        if let Some(Finished {
            fresh,
            result: Ok(worker),
        }) = finished
        {
            self.release_start(task, fresh, &worker);
        }
        taken
    }

    fn message(
        &mut self,
        worker: &WorkerRef,
        intent: &str,
        reply_to: Option<&str>,
        body: &str,
    ) -> Result<(), Failure> {
        let pane = worker
            .pane
            .clone()
            .ok_or_else(|| Failure::task("worker.message", "no pane"))?;
        let _ = reply_to;
        self.deliver(
            &worker.factory,
            Some(&pane),
            delivery::Command::Send {
                target: pane.clone(),
                intent: intent.to_owned(),
                body: body.to_owned(),
                kind: "report".into(),
            },
        )?;
        self.watch(worker);
        Ok(())
    }

    fn sleep(&mut self, worker: &WorkerRef) -> Result<(), Failure> {
        let pane = worker
            .pane
            .clone()
            .ok_or_else(|| Failure::task("worker.sleep", "no pane"))?;
        // The agent is usually still in the turn that reported; the sleep
        // lands once its turn ends.
        if let Ok(mut state) = self.state.lock() {
            state.pending_sleep.insert(pane);
        }
        Ok(())
    }

    fn wake(&mut self, worker: &WorkerRef, body: &str) -> Result<(), Failure> {
        let pane = worker
            .pane
            .clone()
            .ok_or_else(|| Failure::task("worker.wake", "no pane"))?;
        let never_slept = self
            .state
            .lock()
            .map(|mut state| state.pending_sleep.remove(&pane))
            .unwrap_or(false);
        let intent = format!("factory-wake-{}-{}", pane, now_ms());
        if never_slept {
            // Still in its pane: the doorbell rings at its next prompt.
            return self.message(worker, &intent, None, body);
        }
        let runtime = self.runtime()?;
        guard(&runtime).factory_wake(&pane);
        drop(runtime);
        let mut state = self
            .state
            .lock()
            .map_err(|_| Failure::task("worker.wake", "state unavailable"))?;
        let woken = state.waking.entry(pane).or_insert_with(|| Woken {
            worker: worker.clone(),
            letters: Vec::new(),
            since: Instant::now(),
        });
        if woken.letters.len() >= WOKEN_LETTER_LIMIT {
            return Err(Failure::task(
                "worker.wake",
                "too many letters wait for this worker",
            ));
        }
        woken.letters.push((intent, body.to_owned()));
        Ok(())
    }

    fn status(&mut self, worker: &WorkerRef) -> WorkerStatus {
        let Some(pane) = &worker.pane else {
            return WorkerStatus::Gone;
        };
        let pending = self
            .state
            .lock()
            .is_ok_and(|state| state.pending_sleep.contains(pane));
        let Ok(runtime) = self.runtime() else {
            return WorkerStatus::Working;
        };
        let probe = guard(&runtime).factory_worker_probe(pane);
        if probe.asleep || pending {
            return WorkerStatus::Resting {
                since: probe.status_changed_at_unix_ms,
            };
        }
        if !probe.present {
            return if now_ms().saturating_sub(worker.started_at) < START_GRACE_MS {
                WorkerStatus::Working
            } else {
                WorkerStatus::Gone
            };
        }
        if probe.working {
            WorkerStatus::Working
        } else if probe.waiting {
            WorkerStatus::Blocked
        } else {
            WorkerStatus::Resting {
                since: probe.status_changed_at_unix_ms,
            }
        }
    }

    fn stop(&mut self, worker: &WorkerRef) -> Result<(), Failure> {
        // The agent ends through agent sleep once its turn ends, so its pane
        // and session stay for a revive (B50, B55); the ledger record ends now.
        if let Some(pane) = &worker.pane
            && let Ok(mut state) = self.state.lock()
        {
            state.pending_sleep.insert(pane.clone());
        }
        let agent = worker
            .agent
            .as_ref()
            .ok_or_else(|| Failure::task("worker.stop", "worker has no agent"))?;
        let runtime = self.runtime()?;
        let (client, authority, actor) = guard(&runtime)
            .factory_delivery(&worker.factory)
            .map_err(|reason| Failure::task("worker.stop", reason))?;
        drop(runtime);
        crate::coordination::run(
            client,
            authority,
            actor,
            crate::coordination::Command::End {
                id: agent.clone(),
                actor: None,
            },
        )
        .map(|_| ())
        .map_err(|reason| Failure::task("worker.stop", reason))
    }

    fn remove_worktree(&mut self, worker: &WorkerRef, removal: Removal) -> Result<(), Failure> {
        let discard = removal == Removal::Discarded;
        let runtime = self.runtime()?;
        let (connector, node) = {
            let guard = guard(&runtime);
            (
                guard.delivery_connector(guard.node().as_str()),
                guard.own_node(),
            )
        };
        drop(runtime);
        let machine = Machine::new(Arc::clone(&node), Arc::new(AtomicBool::new(false)));
        // The repository the worktree belongs to, read before it goes; a
        // folder already gone leaves nothing to remove.
        let root: Option<String> = machine.call(
            "worktree.root",
            FactoryCall::WorktreeRoot {
                checkout: worker.worktree.clone(),
            },
        )?;
        let connector = connector
            .ok_or_else(|| Failure::environment("worktree", EnvSignal::HerdrSocket, "no Herdr"))?;
        let panes: Vec<String> = worker.pane.iter().cloned().collect();
        // The worker's panes close first so nothing runs in a removed folder.
        crate::live::close_checkout_panes(
            connector.as_ref(),
            node.as_ref(),
            std::slice::from_ref(&worker.worktree),
            &panes,
            crate::live::ProcessWait::for_folder_removal(true),
            crate::live::CONFIRM_TIMEOUT,
        )
        .map_err(|reason| Failure::task("worktree.close", reason))?;
        let Some(root) = root else {
            return Ok(());
        };
        // Leftovers in a finished Task's worktree are the operator's to look
        // at: only a discarded one is forced, with its branch (D-58).
        machine.call(
            "worktree.remove",
            FactoryCall::RemoveWorktree {
                root,
                checkout: worker.worktree.clone(),
                branch: worker.branch.clone(),
                discard,
            },
        )
    }

    fn usage_limited(&mut self, runtime: AgentRuntime) -> Option<UnixMs> {
        let core = self.runtime().ok()?;
        guard(&core).factory_usage_limit(runtime.as_str(), now_ms())
    }
}

/// One worker start through the coordination path, on the starter thread.
fn start_worker(
    runtime: &Weak<Mutex<Runtime>>,
    request: &WorkerSpawn,
) -> Result<WorkerRef, Failure> {
    let runtime = runtime
        .upgrade()
        .ok_or_else(|| Failure::task("worker.spawn", "runtime_gone"))?;
    let (client, authority, actor) = guard(&runtime)
        .factory_delivery(&request.factory)
        .map_err(|reason| Failure::task("worker.spawn", reason))?;
    drop(runtime);
    let parent = crate::coordination::register_code_owned(&client, &authority, &actor)
        .map_err(|reason| Failure::task("worker.spawn", reason))?;
    let mut args = request.args.clone();
    args.push(prompt_argument(&request.prompt, &request.task)?);
    let (intent, path) = match &request.resume {
        Some(previous) => {
            if let Some(agent) = &previous.agent {
                let _ = crate::coordination::run(
                    client.clone(),
                    crate::delivery::worker::Authority {
                        caller: authority.caller.clone(),
                        context: authority.context.clone(),
                    },
                    actor.clone(),
                    crate::coordination::Command::End {
                        id: agent.clone(),
                        actor: None,
                    },
                );
            }
            // Stable per previous worker and attempt, so a start that is
            // asked again continues the same spawn (rule 11).
            (
                format!(
                    "factory-{}-{}-r{}-a{}",
                    request.factory, request.task, previous.started_at, request.attempt
                ),
                Some(previous.worktree.clone()),
            )
        }
        None if request.attempt == 0 => (
            format!("factory-{}-{}", request.factory, request.task),
            None,
        ),
        None => (
            format!(
                "factory-{}-{}-a{}",
                request.factory, request.task, request.attempt
            ),
            None,
        ),
    };
    let view = crate::coordination::run(
        client,
        authority,
        actor,
        crate::coordination::Command::Spawn {
            parent: Some(parent),
            name: request.name.clone(),
            intent,
            kind: request.runtime.as_str().into(),
            repo: request.project.clone(),
            branch: request.branch.clone(),
            path,
            args,
        },
    )
    .map_err(|reason| spawn_failure(&reason))?;
    Ok(WorkerRef {
        factory: request.factory.clone(),
        agent: view["id"].as_str().map(str::to_owned),
        name: request.name.clone(),
        pane: view["pane"].as_str().map(str::to_owned),
        runtime: request.runtime,
        worktree: view["project"].as_str().unwrap_or_default().to_owned(),
        branch: request.branch.clone(),
        started_at: now_ms(),
        asleep: false,
    })
}

fn spawn_failure(reason: &str) -> Failure {
    match reason {
        // The pane was made and the agent typed in; its session is not
        // visible yet. The same intent continues the spawn later.
        "native_identity_unavailable" => Failure::starting("worker.spawn", reason),
        // Another spawn on this machine holds the spawn lock: ask again soon.
        "spawn_busy" => Failure {
            again_in_ms: Some(2_000),
            ..Failure::starting("worker.spawn", reason)
        },
        "delivery_unavailable" | "ledger_unavailable" => {
            Failure::environment("worker.spawn", EnvSignal::HerdrSocket, reason)
        }
        // A Herdr request that could not reach the server.
        _ if reason.contains("socket") || reason.contains("connect") => {
            Failure::environment("worker.spawn", EnvSignal::HerdrSocket, reason)
        }
        _ => Failure::task("worker.spawn", reason),
    }
}

/// The worker's first prompt as the one argument Herdr types into the pane
/// (`live::prompt_argument`), short enough for the spawn's argument limit;
/// the full card is one `hide factory show` away.
fn prompt_argument(prompt: &str, task: &str) -> Result<String, Failure> {
    let clean: String = prompt
        .chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut limit = PROMPT_LIMIT;
    loop {
        let text = if clean.len() <= limit {
            clean.clone()
        } else {
            let mut end = limit;
            while !clean.is_char_boundary(end) {
                end -= 1;
            }
            format!(
                "{}\n\n(이어지는 내용은 `hide factory show {task}`로 읽으세요.)",
                &clean[..end]
            )
        };
        match crate::live::prompt_argument(&text) {
            Ok(argument) => return Ok(argument),
            Err(_) if limit > 1024 => limit /= 2,
            Err(reason) => return Err(Failure::task("worker.prompt", reason)),
        }
    }
}

/// Local issues (`L-<n>`) live in the core's local issue store (B3).
struct CoreIssues {
    runtime: Weak<Mutex<Runtime>>,
}

impl IssueBook for CoreIssues {
    fn create(
        &mut self,
        project: &str,
        title: &str,
        body: &str,
        marker: &str,
    ) -> Result<u32, Failure> {
        let runtime = lock(&self.runtime).ok_or_else(|| Failure::task("issue", "core stopped"))?;
        let number = guard(&runtime).factory_local_issue(project, title, body, marker);
        number.map_err(|reason| Failure::task("issue.create", reason))
    }

    fn read(
        &mut self,
        project: &str,
        number: u32,
    ) -> Result<hide_factory::adapters::IssueText, Failure> {
        let runtime = lock(&self.runtime).ok_or_else(|| Failure::task("issue", "core stopped"))?;
        let issue = guard(&runtime).factory_local_issue_read(project, number);
        let issue = issue.ok_or_else(|| Failure::task("issue.read", "missing"))?;
        Ok(hide_factory::adapters::IssueText {
            title: issue.title,
            body: issue.body,
            open: issue.open,
        })
    }
}

/// The machine the Factory's projects live on, as its node reports it.
struct MachineEnvironment(Machine);

impl Environment for MachineEnvironment {
    fn disk_free(&mut self, project: &str) -> Option<u64> {
        self.0
            .node_call::<Option<u64>>(
                "disk",
                Call::VolumeFree {
                    path: project.to_owned(),
                },
            )
            .ok()
            .flatten()
    }

    /// A reading the node cannot give is normal pressure, as on a system
    /// without one.
    fn memory_pressure(&mut self) -> MemoryPressure {
        match self
            .0
            .call::<NodeMemoryPressure>("memory", FactoryCall::MemoryPressure)
        {
            Ok(NodeMemoryPressure::Critical) => MemoryPressure::Critical,
            Ok(NodeMemoryPressure::Warn) => MemoryPressure::Warn,
            Ok(NodeMemoryPressure::Normal) | Err(_) => MemoryPressure::Normal,
        }
    }
}

struct CoreNotifier {
    runtime: Weak<Mutex<Runtime>>,
}

impl Notifier for CoreNotifier {
    fn macos(&mut self, title: &str, body: &str) {
        let Some(runtime) = lock(&self.runtime) else {
            return;
        };
        let connector = {
            let guard = guard(&runtime);
            guard.delivery_connector(guard.node().as_str())
        };
        drop(runtime);
        if let Some(connector) = connector {
            let _ = hide_herdr_client::request_small_response(
                connector.as_ref(),
                "notification.show",
                json!({"title": title, "body": body}),
                Duration::from_millis(500),
            );
        }
    }

    /// A pending review's result goes to the pane that added the Task (B11).
    fn producer(&mut self, factory: &str, pane: &str, body: &str) -> bool {
        let Some(runtime) = lock(&self.runtime) else {
            return false;
        };
        let prepared = guard(&runtime).factory_prepare(
            factory,
            Some(pane),
            delivery::Command::Send {
                target: pane.to_owned(),
                intent: format!(
                    "factory-review-{}",
                    &hide_factory::store::sha256_hex(body.as_bytes())[..16]
                ),
                body: body.to_owned(),
                kind: "report".into(),
            },
        );
        drop(runtime);
        prepared
            .and_then(|prepared| prepared.run(DELIVERY_TIMEOUT))
            .is_ok()
    }
}

/// The Factory's own judgment queue on its own router (D-13, D-44): one in
/// flight, intake reviews first, 16 waiting per Factory; a full queue or a
/// failed provider is answered as failed, never skipped (B19, B68).
struct JudgeThread {
    shared: Arc<JudgeShared>,
    thread: Option<JoinHandle<()>>,
}

struct JudgeShared {
    queue: Mutex<VecDeque<Judgment>>,
    answers: Mutex<Vec<JudgmentAnswer>>,
    wake: std::sync::Condvar,
    stop: AtomicBool,
    cancel: hide_ai::CancelToken,
}

impl JudgeThread {
    fn start(runtime: Weak<Mutex<Runtime>>, home: Option<PathBuf>) -> Self {
        let shared = Arc::new(JudgeShared {
            queue: Mutex::new(VecDeque::new()),
            answers: Mutex::new(Vec::new()),
            wake: std::sync::Condvar::new(),
            stop: AtomicBool::new(false),
            cancel: hide_ai::CancelToken::new(),
        });
        let worker = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("herdr-core-factory-judge".into())
            .spawn(move || judge_loop(worker, runtime, home))
            .ok();
        Self { shared, thread }
    }

    fn port(&self) -> JudgePort {
        JudgePort {
            shared: Arc::clone(&self.shared),
            alive: self.thread.is_some(),
        }
    }
}

impl Drop for JudgeThread {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.cancel.cancel();
        self.shared.wake.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct JudgePort {
    shared: Arc<JudgeShared>,
    alive: bool,
}

impl Judge for JudgePort {
    fn submit(&mut self, judgment: Judgment) -> Result<(), Failure> {
        if !self.alive {
            return Err(Failure::task("judge", "the judgment thread did not start"));
        }
        let mut queue = self
            .shared
            .queue
            .lock()
            .map_err(|_| Failure::task("judge", "queue poisoned"))?;
        let waiting = queue
            .iter()
            .filter(|queued| queued.factory == judgment.factory)
            .count();
        if waiting >= hide_factory::judgment::QUEUE_LIMIT {
            return Err(Failure::task(
                "judge",
                "the Factory's judgment queue is full",
            ));
        }
        // Intake reviews go ahead of every other Factory judgment (D-44).
        let position = queue
            .iter()
            .position(|queued| queued.priority > judgment.priority)
            .unwrap_or(queue.len());
        queue.insert(position, judgment);
        self.shared.wake.notify_one();
        Ok(())
    }

    fn finished(&mut self) -> Vec<JudgmentAnswer> {
        self.shared
            .answers
            .lock()
            .map(|mut answers| std::mem::take(&mut *answers))
            .unwrap_or_default()
    }
}

fn judge_loop(shared: Arc<JudgeShared>, runtime: Weak<Mutex<Runtime>>, home: Option<PathBuf>) {
    let mut router: Option<(hide_ai::AiSettings, hide_ai::AiRouter)> = None;
    loop {
        let judgment = {
            let Ok(mut queue) = shared.queue.lock() else {
                return;
            };
            loop {
                if shared.stop.load(Ordering::Acquire) {
                    return;
                }
                if let Some(judgment) = queue.pop_front() {
                    break judgment;
                }
                queue = match shared.wake.wait_timeout(queue, Duration::from_secs(5)) {
                    Ok((queue, _)) => queue,
                    Err(_) => return,
                };
            }
        };
        // The core ended: nothing is left to answer the judgment to.
        let Some(core) = lock(&runtime) else { return };
        let (settings, node) = {
            let core = guard(&core);
            (core.factory_ai_settings(), core.own_node())
        };
        drop(core);
        let settings = settings
            .or_else(|| {
                home.as_deref()
                    .and_then(|home| hide_ai::settings::load(home).ok())
            })
            .unwrap_or_default();
        if router
            .as_ref()
            .is_none_or(|(current, _)| *current != settings)
        {
            router = Some((
                settings.clone(),
                crate::ai::factory_router(&node, &settings),
            ));
        }
        let Some((_, router)) = &router else { continue };
        let request = hide_ai::AiRequest {
            feature_id: judgment.feature_id().into(),
            request_id: hide_ai::RequestId(judgment.id.clone()),
            subject_id: judgment
                .task
                .clone()
                .unwrap_or_else(|| judgment.factory.clone()),
            system: judgment.system().to_owned(),
            input: judgment.render_input(),
            output_schema: judgment.schema(),
            deadline: JUDGMENT_DEADLINE,
            schema_version: hide_factory::judgment::SCHEMA_VERSION.into(),
            pick: None,
        };
        let started = Instant::now();
        let outcome = match router.execute(&request, &shared.cancel) {
            Ok(result) => JudgmentOutcome::Answered {
                value: result.value,
            },
            Err(error) => JudgmentOutcome::Failed {
                reason: error.class().to_owned(),
            },
        };
        crate::diagnostic!(json!({"component":"factory","kind":"judgment.finished",
            "feature":request.feature_id,"factory":judgment.factory,"task":judgment.task,
            "ok":matches!(outcome, JudgmentOutcome::Answered { .. }),"elapsed_ms":started.elapsed().as_millis() as u64}));
        if let Ok(mut answers) = shared.answers.lock() {
            answers.push(JudgmentAnswer {
                id: judgment.id.clone(),
                factory: judgment.factory.clone(),
                task: judgment.task.clone(),
                outcome,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_access::{LinkAnswer, LinkError, NodeLink};
    use std::cell::RefCell;

    fn question_caller(herdr: &crate::fake_herdr::FakeHerdr) -> QuestionCaller {
        let actor = delivery::Actor {
            pane_id: "w1:p1".into(),
            name: "w1:p1".into(),
            kind: "codex".into(),
            device_id: crate::node::TEST_NODE.into(),
            session: crate::wire::session_digest("native-1"),
        };
        QuestionCaller {
            context: crate::runtime::delivery::tests::authority(&actor).context,
            actor,
            raw_pane: "w1:p1".into(),
            terminal_id: "terminal-1".into(),
            connector: Arc::new(herdr.connector()),
        }
    }

    fn question_herdr(native: &str) -> crate::fake_herdr::FakeHerdr {
        let native = native.to_owned();
        crate::fake_herdr::FakeHerdr::start("question-guard", move |method, params| {
            assert_eq!(method, "agent.get");
            assert_eq!(params["target"], "w1:p1");
            json!({"type":"agent_info", "agent":{
                "pane_id":"w1:p1", "tab_id":"w1:t1", "workspace_id":"w1",
                "terminal_id":"terminal-1", "agent":"codex", "agent_status":"working",
                "state_change_seq":1, "focused":false, "revision":1,
                "agent_session":{"source":"herdr:codex", "agent":"codex", "kind":"id", "value":native},
            }})
        })
    }

    #[test]
    fn a_question_cannot_start_an_unstarted_factory_or_create_its_store() {
        let root = tempfile::tempdir().unwrap();
        let herdr = question_herdr("native-1");
        let mut host = FactoryHost::start(
            root.path(),
            Some(root.path().into()),
            Weak::new(),
            ChangeNotifier::noop(),
        )
        .unwrap();
        let answer = host
            .prepare_question_guard(
                question_caller(&herdr),
                Weak::new(),
                Instant::now() + Duration::from_secs(5),
            )
            .run();
        assert_eq!(answer, Err("factory_guard_unstarted".into()));
        host.shutdown();
        assert!(!hide_kit::layout::factory_store(root.path()).exists());
        assert!(!hide_kit::layout::factory_files(root.path()).exists());
    }

    #[test]
    fn expired_replaced_and_overloaded_question_checks_never_deny() {
        let herdr = question_herdr("native-2");
        let (requests, _receiver) = mpsc::sync_channel(0);
        let prepared = |deadline| PreparedQuestionGuard {
            requests: requests.clone(),
            caller: question_caller(&herdr),
            runtime: Weak::new(),
            deadline,
        };
        assert_eq!(
            prepared(Instant::now()).run(),
            Err("factory_guard_expired".into())
        );
        assert!(
            herdr.requests().is_empty(),
            "expired work never opens a native read"
        );
        assert_eq!(
            prepared(Instant::now() + Duration::from_secs(5)).run(),
            Err("factory_guard_native_changed".into())
        );
        let current = question_herdr("native-1");
        let mut stale_terminal = question_caller(&current);
        stale_terminal.terminal_id = "replaced-terminal".into();
        assert_eq!(
            PreparedQuestionGuard {
                requests: requests.clone(),
                caller: stale_terminal,
                runtime: Weak::new(),
                deadline: Instant::now() + Duration::from_secs(5),
            }
            .run(),
            Err("factory_guard_native_changed".into()),
            "native session text on a reused pane cannot lend the prior terminal's authority"
        );
        let busy = PreparedQuestionGuard {
            requests,
            caller: question_caller(&current),
            runtime: Weak::new(),
            deadline: Instant::now() + Duration::from_secs(5),
        };
        assert_eq!(
            busy.run(),
            Err("factory_guard_busy".into()),
            "a full queue answers immediately"
        );
    }

    #[test]
    fn an_unopened_factory_keeps_its_idle_wait_after_every_due_tick() {
        let started = Instant::now();
        for ticks in 0..100 {
            assert_eq!(request_wait(None, started, started + TICK * ticks), TICK);
        }
    }

    /// This node is unavailable throughout the test; the already-open engine
    /// must still apply its completed provider answer and settle the caller.
    struct SchedulerNode;

    impl NodeLink for SchedulerNode {
        fn call(&self, _call: Call, _timeout: Duration) -> Result<LinkAnswer, LinkError> {
            Err(LinkError::Busy)
        }

        fn call_with_progress(
            &self,
            call: Call,
            timeout: Duration,
            _progress: &mut dyn FnMut(Value) -> bool,
        ) -> Result<LinkAnswer, LinkError> {
            self.call(call, timeout)
        }
    }

    #[test]
    fn sustained_readonly_questions_cannot_starve_a_due_review_or_its_waiter() {
        // A constant clock proves reads alone do not apply an available
        // judgment. Advancing it crosses multiple ticks with no channel gap.
        for advance in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let project = root.path().join("project").to_string_lossy().into_owned();
            let paths = Paths {
                store: root.path().join("factory.sqlite3"),
                files: root.path().join("factory-files"),
            };
            let factory = hide_factory::model::Factory {
                id: "f-1".into(),
                project: project.clone(),
                project_name: "project".into(),
                source: hide_factory::model::SourceKind::Local,
                repo: None,
                default_branch: "main".into(),
                config: Default::default(),
                closed: false,
                created_at: now_ms(),
                next_task: 1,
                next_local_issue: 1,
                main: Default::default(),
                outside_read_at: Some(now_ms()),
                outside_read_failures: 0,
                watch_day: 0,
                watch_sent_today: 0,
                watch_last_at: Some(now_ms()),
                github_approval: None,
            };
            hide_factory::store::Store::open(&paths.store, &paths.files)
                .unwrap()
                .put_factory(&factory)
                .unwrap();
            let runtime = Weak::new();
            let workers = Arc::new(Mutex::new(WorkerState::default()));
            let machine = Machine::new(Arc::new(SchedulerNode), Arc::new(AtomicBool::new(false)));
            let projects = SharedProjects::new(
                machine.clone(),
                Box::new(CoreIssues {
                    runtime: runtime.clone(),
                }),
                paths.files.join("logs"),
            );
            let judgments = Arc::new(JudgeShared {
                queue: Mutex::new(VecDeque::new()),
                answers: Mutex::new(Vec::new()),
                wake: std::sync::Condvar::new(),
                stop: AtomicBool::new(false),
                cancel: hide_ai::CancelToken::new(),
            });
            let mut engine = Some(
                Engine::open(
                    &paths.store,
                    &paths.files,
                    Ports {
                        clock: Box::new(SystemClock::new(machine.clone())),
                        source: Box::new(projects.clone()),
                        verifier: Box::new(projects.clone()),
                        merge: Box::new(projects),
                        workers: Box::new(CoreWorkers {
                            runtime: runtime.clone(),
                            state: Arc::clone(&workers),
                        }),
                        judge: Box::new(JudgePort {
                            shared: Arc::clone(&judgments),
                            alive: true,
                        }),
                        environment: Box::new(MachineEnvironment(machine)),
                        notifier: Box::new(CoreNotifier {
                            runtime: runtime.clone(),
                        }),
                    },
                )
                .unwrap(),
            );
            let (send, requests) = mpsc::sync_channel(QUEUE_LIMIT);
            let (reply, answer) = mpsc::sync_channel(1);
            send.try_send(Request::Command {
                caller: FactoryCaller {
                    pane: None,
                    cwd: Some(project.clone()),
                    claimed: None,
                    ancestors: Lineage::none(),
                },
                command: Command::Add {
                    project: Some(project),
                    task: None,
                    issue: None,
                    card: hide_factory::command::CardInput {
                        title: Some("A scheduled review".into()),
                        goal: Some("Settle the review while read-only questions arrive".into()),
                        criteria: vec!["The adding caller receives its review answer".into()],
                        ..Default::default()
                    },
                    producer_pane: None,
                },
                reply,
            })
            .unwrap_or_else(|_| panic!("the Add fits the queue"));
            let herdr = question_herdr("native-1");
            let mut guard_answers = Vec::new();
            for _ in 0..12 {
                let (reply, answer) = mpsc::sync_channel(1);
                send.try_send(Request::QuestionGuard {
                    caller: question_caller(&herdr),
                    runtime: runtime.clone(),
                    deadline: Instant::now() + Duration::from_secs(60),
                    reply,
                })
                .unwrap_or_else(|_| panic!("the guards fit the queue"));
                guard_answers.push(answer);
            }
            drop(send);
            let base = Instant::now();
            let mut elapsed = Duration::ZERO;
            run_requests(
                &mut engine,
                &mut Publisher::default(),
                &mut RuntimeSink {
                    runtime,
                    notifier: ChangeNotifier::noop(),
                },
                &workers,
                &requests,
                &AtomicBool::new(false),
                || panic!("an already-open Factory is never reopened"),
                || {
                    // Release the external provider's completed intake as
                    // soon as Add has submitted it, independently of ticks.
                    for judgment in judgments.queue.lock().unwrap().drain(..) {
                        judgments.answers.lock().unwrap().push(JudgmentAnswer {
                            id: judgment.id,
                            factory: judgment.factory,
                            task: judgment.task,
                            outcome: JudgmentOutcome::Answered {
                                value: json!({
                                    "questions": [{"text":"Which behavior?", "suggestion":"Keep the current behavior"}],
                                    "dependencies":[], "split":[], "flags":[],
                                }),
                            },
                        });
                    }
                    let now = base + elapsed;
                    if advance {
                        elapsed += TICK / 4;
                    }
                    now
                },
            );
            for guard in guard_answers {
                assert_eq!(guard.try_recv().unwrap(), Ok(false));
            }
            if advance {
                assert!(elapsed > 2 * TICK, "reads span multiple scheduled ticks");
                let answer = answer.try_recv().expect("the Add waiter must settle");
                assert_eq!(answer["result"], "needs_answers", "{answer}");
                assert_eq!(answer["questions"][0]["text"], "Which behavior?");
            } else {
                assert!(
                    answer.try_recv().is_err(),
                    "reads do not settle the review early"
                );
                assert!(matches!(
                    engine.as_ref().unwrap().task("f-1", "T-1").unwrap().review,
                    hide_factory::model::ReviewState::Requested { .. }
                ));
            }
        }
    }

    /// The core's own node answering the offset in turn, then busy.
    struct Offsets(Mutex<Vec<Result<i64, ()>>>);

    impl NodeLink for Offsets {
        fn call(&self, call: Call, _timeout: Duration) -> Result<LinkAnswer, LinkError> {
            assert!(matches!(
                call,
                Call::Factory {
                    call: FactoryCall::UtcOffset
                }
            ));
            match self.0.lock().unwrap().remove(0) {
                Ok(offset) => Ok(LinkAnswer::Parsed(json!(offset))),
                Err(()) => Err(LinkError::Busy),
            }
        }

        fn call_with_progress(
            &self,
            call: Call,
            timeout: Duration,
            _progress: &mut dyn FnMut(serde_json::Value) -> bool,
        ) -> Result<LinkAnswer, LinkError> {
            self.call(call, timeout)
        }
    }

    fn clock(answers: Vec<Result<i64, ()>>) -> SystemClock {
        SystemClock::new(Machine::new(
            Arc::new(Offsets(Mutex::new(answers))),
            Arc::new(AtomicBool::new(false)),
        ))
    }

    /// A node too busy to answer keeps the day where it was: Seoul stays nine
    /// hours east through a busy read, and only a clock that never heard is UTC.
    #[test]
    fn a_busy_node_keeps_the_last_offset_it_gave() {
        const SEOUL: i64 = 9 * 3_600_000;
        let heard = clock(vec![Ok(SEOUL), Err(()), Err(()), Ok(SEOUL - 3_600_000)]);
        assert_eq!(heard.utc_offset_ms(), SEOUL);
        assert_eq!(heard.utc_offset_ms(), SEOUL);
        assert_eq!(heard.utc_offset_ms(), SEOUL);
        assert_eq!(
            heard.utc_offset_ms(),
            SEOUL - 3_600_000,
            "a new answer is taken"
        );
        assert_eq!(clock(vec![Err(())]).utc_offset_ms(), 0);
    }

    #[test]
    fn the_same_report_after_the_task_moved_is_a_new_letter() {
        let body = r#"{"factory":{"done":{}}}"#;
        assert_eq!(
            report_intent("w1:p1", 10, body),
            report_intent("w1:p1", 10, body),
            "a retry in the same state is one letter"
        );
        assert_ne!(
            report_intent("w1:p1", 10, body),
            report_intent("w1:p1", 20, body),
            "done again after verification sent the Task back"
        );
        assert_ne!(
            report_intent("w1:p1", 10, body),
            report_intent("w2:p1", 10, body)
        );
    }

    fn request(task: &str, resume: Option<WorkerRef>) -> WorkerSpawn {
        WorkerSpawn {
            factory: "f-1".into(),
            task: task.into(),
            name: format!("w-{task}"),
            runtime: hide_factory::model::Runtime::Claude,
            project: "/work/p".into(),
            branch: format!("factory/{task}"),
            prompt: "p".into(),
            args: Vec::new(),
            resume,
            attempt: 0,
        }
    }

    fn worker(job: &WorkerSpawn) -> WorkerRef {
        WorkerRef {
            factory: job.factory.clone(),
            agent: Some(format!("agent-{}", job.task)),
            name: job.name.clone(),
            pane: Some(format!("pane-{}", job.task)),
            runtime: job.runtime,
            worktree: format!("/work/p.worktrees/{}", job.task),
            branch: job.branch.clone(),
            started_at: 1,
            asleep: false,
        }
    }

    /// A starter state with a queue of `capacity`, and the queue's far end.
    fn starter(capacity: usize) -> (Arc<Mutex<WorkerState>>, Receiver<WorkerSpawn>) {
        let state = Arc::new(Mutex::new(WorkerState::default()));
        let (queue, jobs) = mpsc::sync_channel(capacity);
        state.lock().unwrap().starts.queue = Some(queue);
        (state, jobs)
    }

    fn close(state: &Mutex<WorkerState>) {
        state.lock().unwrap().starts.queue = None;
    }

    #[test]
    fn a_start_is_pending_until_the_starter_finishes_and_is_answered_once() {
        let (state, jobs) = starter(4);
        let job = request("T-1", None);
        let key = start_key("f-1", "T-1");
        assert_eq!(poll_start(&state, &key).map(|r| r.is_ok()), None);
        let pending = queue_start(&state, &job);
        assert_eq!((pending.starting, pending.again_in_ms), (true, Some(0)));
        assert!(matches!(poll_start(&state, &key), Some(Err(f)) if f.starting));
        close(&state);
        run_starts(
            jobs,
            Arc::clone(&state),
            Arc::new(AtomicBool::new(false)),
            |job| Ok(worker(job)),
            |_, _| panic!("nothing was abandoned"),
        );
        let answer = poll_start(&state, &key).unwrap().unwrap();
        assert_eq!(answer.agent.as_deref(), Some("agent-T-1"));
        assert!(poll_start(&state, &key).is_none(), "answered once");
    }

    #[test]
    fn a_full_queue_is_asked_again_later_and_never_waited_on() {
        let (state, _jobs) = starter(1);
        queue_start(&state, &request("T-1", None));
        let full = queue_start(&state, &request("T-2", None));
        assert!(full.starting);
        assert!(
            poll_start(&state, &start_key("f-1", "T-2")).is_none(),
            "not queued, so the next tick queues it"
        );
        close(&state);
        let stopped = queue_start(&state, &request("T-3", None));
        assert_eq!(stopped.detail, "starter_stopped");
        assert!(
            stopped.starting,
            "a stopping host is not the Task's failure"
        );
    }

    #[test]
    fn an_abandoned_start_releases_its_worker_and_a_new_worktree() {
        let (state, jobs) = starter(4);
        let resumed = request("T-2", Some(worker(&request("T-2", None))));
        queue_start(&state, &request("T-1", None));
        queue_start(&state, &resumed);
        {
            let mut state = state.lock().unwrap();
            state.starts.abandoned.insert(start_key("f-1", "T-1"));
            state.starts.abandoned.insert(start_key("f-1", "T-2"));
        }
        close(&state);
        let released = RefCell::new(Vec::new());
        run_starts(
            jobs,
            Arc::clone(&state),
            Arc::new(AtomicBool::new(false)),
            |job| Ok(worker(job)),
            |job, worker| {
                released
                    .borrow_mut()
                    .push((worker.agent.clone().unwrap(), job.resume.is_none()))
            },
        );
        assert_eq!(
            released.into_inner(),
            vec![("agent-T-1".into(), true), ("agent-T-2".into(), false)],
            "a resumed worker keeps the Task's worktree"
        );
        let state = state.lock().unwrap();
        assert!(state.starts.done.is_empty() && state.starts.in_flight.is_empty());
        assert!(state.starts.abandoned.is_empty());
    }

    #[test]
    fn a_stopped_host_drops_the_starts_still_queued() {
        let (state, jobs) = starter(4);
        queue_start(&state, &request("T-1", None));
        queue_start(&state, &request("T-2", None));
        close(&state);
        let started = RefCell::new(0);
        run_starts(
            jobs,
            Arc::clone(&state),
            Arc::new(AtomicBool::new(true)),
            |job| {
                *started.borrow_mut() += 1;
                Ok(worker(job))
            },
            |_, _| {},
        );
        assert_eq!(started.into_inner(), 0);
        assert!(state.lock().unwrap().starts.in_flight.is_empty());
    }

    fn judgment(factory: &str, id: &str, priority: hide_factory::judgment::Priority) -> Judgment {
        Judgment {
            id: id.into(),
            factory: factory.into(),
            task: None,
            priority,
            input: hide_factory::judgment::JudgmentInput::Watch { board: json!({}) },
        }
    }

    #[test]
    fn intake_reviews_go_first_and_each_factory_queues_at_most_its_limit() {
        use hide_factory::judgment::{Priority, QUEUE_LIMIT};
        let shared = Arc::new(JudgeShared {
            queue: Mutex::new(VecDeque::new()),
            answers: Mutex::new(Vec::new()),
            wake: std::sync::Condvar::new(),
            stop: AtomicBool::new(false),
            cancel: hide_ai::CancelToken::new(),
        });
        let mut port = JudgePort {
            shared: Arc::clone(&shared),
            alive: true,
        };
        port.submit(judgment("f-1", "watch", Priority::Factory))
            .unwrap();
        port.submit(judgment("f-1", "intake", Priority::Intake))
            .unwrap();
        port.submit(judgment("f-1", "drift", Priority::Factory))
            .unwrap();
        let order: Vec<String> = shared
            .queue
            .lock()
            .unwrap()
            .iter()
            .map(|j| j.id.clone())
            .collect();
        assert_eq!(order, vec!["intake", "watch", "drift"]);
        for n in 3..QUEUE_LIMIT {
            port.submit(judgment("f-1", &format!("j{n}"), Priority::Factory))
                .unwrap();
        }
        let full = port
            .submit(judgment("f-1", "one more", Priority::Intake))
            .unwrap_err();
        assert_eq!(full.detail, "the Factory's judgment queue is full");
        port.submit(judgment("f-2", "another factory", Priority::Factory))
            .expect("the limit is per Factory");
        let mut dead = JudgePort {
            shared,
            alive: false,
        };
        assert!(dead.submit(judgment("f-3", "x", Priority::Intake)).is_err());
    }

    #[test]
    fn a_busy_spawn_lock_is_asked_again_soon() {
        let busy = spawn_failure("spawn_busy");
        assert!(busy.starting && busy.signal.is_none());
        assert_eq!(busy.again_in_ms, Some(2_000));
        assert!(!spawn_failure("worktree_create_failed").starting);
    }
}
