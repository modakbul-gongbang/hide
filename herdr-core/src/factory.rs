//! The Software Factory host: the core owns one engine thread that runs every
//! Factory on this machine (PRD software-factory, Technical structure). The
//! engine never runs under `Mutex<Runtime>`; it takes the lock only to read
//! owned values or to ask the runtime for a delivery, a sleep or a wake.
//!
//! The engine opens its store the first time it is asked, or at start when a
//! store already exists, so a machine without a Factory pays nothing.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hide_factory::adapters::{
    Clock, EnvSignal, Environment, Failure, Judge, MemoryPressure, Notifier, WorkerRuntime,
    WorkerSpawn, WorkerStatus,
};
use hide_factory::exec::SystemRunner;
use hide_factory::judgment::{Judgment, JudgmentAnswer, JudgmentOutcome};
use hide_factory::model::{Runtime as AgentRuntime, UnixMs, WorkerRef};
use hide_factory::project::{IssueBook, SharedProjects};
use hide_factory::role::Role;
use hide_factory::{Command, Engine, Inbound, Ports, Refusal};
use serde_json::{Value, json};

use crate::delivery;
use crate::runtime::Runtime;

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
}

enum Request {
    Command {
        caller: FactoryCaller,
        command: Command,
        reply: SyncSender<Value>,
    },
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
            .spawn(move || run(paths, home, runtime, receiver, thread_stop))
            .map_err(|error| format!("factory engine thread could not start: {error}"))?;
        Ok(Self {
            requests,
            stop,
            thread: Some(thread),
        })
    }

    pub(crate) fn prepare(&self, caller: FactoryCaller, command: Command) -> PreparedFactory {
        PreparedFactory {
            requests: self.requests.clone(),
            caller,
            command,
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
    requests: Receiver<Request>,
    stop: Arc<AtomicBool>,
) {
    let workers = Arc::new(Mutex::new(WorkerState::default()));
    let runner_stop = Arc::clone(&stop);
    let mut engine: Option<Engine> = None;
    let mut judge: Option<JudgeThread> = None;
    let open = |judge: &mut Option<JudgeThread>| -> Option<Engine> {
        let started = JudgeThread::start(runtime.clone(), home.clone());
        let port = started.port();
        *judge = Some(started);
        let projects = SharedProjects::new(
            Box::new(SystemRunner {
                stop: Arc::clone(&runner_stop),
            }),
            Box::new(CoreIssues {
                runtime: runtime.clone(),
            }),
            paths.files.join("logs"),
        );
        let ports = Ports {
            clock: Box::new(SystemClock),
            source: Box::new(projects.clone()),
            verifier: Box::new(projects.clone()),
            merge: Box::new(projects),
            workers: Box::new(CoreWorkers {
                runtime: runtime.clone(),
                state: Arc::clone(&workers),
            }),
            judge: Box::new(port),
            environment: Box::new(MachineEnvironment),
            notifier: Box::new(CoreNotifier {
                runtime: runtime.clone(),
            }),
        };
        match Engine::open(&paths.store, &paths.files, ports) {
            Ok(mut engine) => {
                if let Some(program) = hide_program() {
                    engine.set_hide_program(program);
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
    let mut waiters: Vec<Waiter> = Vec::new();
    let mut last_tick = Instant::now();
    while !stop.load(Ordering::Acquire) {
        match requests.recv_timeout(TICK) {
            Ok(Request::Command {
                caller,
                command,
                reply,
            }) => {
                if engine.is_none() {
                    engine = open(&mut judge);
                }
                let Some(engine) = engine.as_mut() else {
                    let _ = reply.send(
                        Refusal::new("factory_unavailable", "See the diagnostic log").to_json(),
                    );
                    continue;
                };
                let added = matches!(command, Command::Add { .. });
                let answer = handle(engine, &runtime, &caller, command);
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
                        until: Instant::now() + REVIEW_WAIT,
                    });
                    continue;
                }
                let _ = reply.send(answer);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let Some(engine) = engine.as_mut() else {
            continue;
        };
        if last_tick.elapsed() >= TICK {
            last_tick = Instant::now();
            pump_letters(engine, &runtime);
            engine.tick();
            settle_sleeps(&workers, &runtime);
            let mut port = CoreWorkers {
                runtime: runtime.clone(),
                state: Arc::clone(&workers),
            };
            port.deliver_woken();
            publish_recipients(Some(engine), &runtime);
        }
        waiters.retain(|waiter| {
            let settled = engine.review_settled(&waiter.factory, &waiter.task);
            if settled || Instant::now() >= waiter.until {
                let _ = waiter
                    .reply
                    .send(engine.add_answer(&waiter.factory, &waiter.task));
                return false;
            }
            true
        });
    }
    drop(engine);
    drop(judge);
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
    let ids: BTreeSet<String> = engine
        .into_iter()
        .flat_map(Engine::factories)
        .filter(|factory| !factory.closed)
        .map(|factory| factory.id.clone())
        .collect();
    if let Some(runtime) = lock(runtime) {
        guard(&runtime).set_factory_recipients(ids);
    }
}

/// Runs one command with the caller's role (D-33). A worker's report travels
/// as a ledger letter from its own pane, so a harness that only speaks the
/// letter protocol lands on the same path (B25).
/// The `hide` program beside the running daemon (the app bundle's
/// Resources, or a build's target folder).
fn hide_program() -> Option<String> {
    let name = if cfg!(windows) { "hide.exe" } else { "hide" };
    let program = std::env::current_exe().ok()?.parent()?.join(name);
    program
        .is_file()
        .then(|| program.to_string_lossy().into_owned())
}

fn handle(
    engine: &mut Engine,
    runtime: &Weak<Mutex<Runtime>>,
    caller: &FactoryCaller,
    command: Command,
) -> Value {
    let role = match engine.role_for(caller.pane.as_deref(), caller.cwd.as_deref()) {
        Some((factory, task)) => Role::Worker { factory, task },
        None => Role::Operator {
            pane: caller.pane.clone().unwrap_or_else(|| "checkout".into()),
        },
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
    let (Role::Worker { factory, .. }, Some(kind), Some(pane)) = (&role, kind, &caller.pane) else {
        return engine.command(&role, command);
    };
    let body = json!({"factory": command}).to_string();
    let intent = format!(
        "factory-{}",
        &hide_factory::store::sha256_hex(format!("{pane}\n{body}").as_bytes())[..24]
    );
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

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> UnixMs {
        now_ms()
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
    fn spawn(&mut self, request: &WorkerSpawn) -> Result<WorkerRef, Failure> {
        let runtime = self.runtime()?;
        if request.runtime == hide_factory::model::Runtime::Codex
            && !guard(&runtime).factory_kit_read()
        {
            return Err(Failure::starting("worker.spawn", "kit_not_read"));
        }
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
                (
                    format!("factory-{}-{}-{}", request.factory, request.task, now_ms()),
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
                parent,
                name: request.name.clone(),
                intent,
                kind: request.runtime.as_str().into(),
                repo: request.project.clone(),
                branch: request.branch.clone(),
                path,
                no_watch: false,
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
        let Some(agent) = &worker.agent else {
            return Ok(());
        };
        if let Some(pane) = &worker.pane
            && let Ok(mut state) = self.state.lock()
        {
            state.pending_sleep.remove(pane);
        }
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

    fn remove_worktree(&mut self, worker: &WorkerRef, delete_branch: bool) -> Result<(), Failure> {
        let checkout = Path::new(&worker.worktree);
        let mut runner = SystemRunner {
            stop: Arc::new(AtomicBool::new(false)),
        };
        // The repository the worktree belongs to, read before it goes; a
        // folder already gone leaves only the branch to delete.
        let root = match checkout.exists() {
            true => Some(repository_root(&mut runner, checkout)?),
            false => None,
        };
        let runtime = self.runtime()?;
        let connector = guard(&runtime).delivery_connector("local");
        drop(runtime);
        let connector = connector
            .ok_or_else(|| Failure::environment("worktree", EnvSignal::HerdrSocket, "no Herdr"))?;
        let panes: Vec<String> = worker.pane.iter().cloned().collect();
        // The worker's panes close first so nothing runs in a removed folder.
        crate::live::close_checkout_panes(
            connector.as_ref(),
            std::slice::from_ref(&worker.worktree),
            &panes,
            crate::live::ProcessWait::for_folder_removal(true),
            crate::live::CONFIRM_TIMEOUT,
        )
        .map_err(|reason| Failure::task("worktree.close", reason))?;
        let Some(root) = root else {
            return Ok(());
        };
        // The Factory made this worktree, so what is left in it goes too.
        hide_host::worktrees::remove_worktree(&root, checkout, true)
            .map_err(|reason| Failure::task("worktree.remove", reason))?;
        if delete_branch {
            hide_factory::exec::checked(
                &mut runner,
                "branch.delete",
                "git",
                &["branch", "-D", "--", &worker.branch],
                Some(&root),
            )?;
        }
        Ok(())
    }

    fn usage_limited(&mut self, _runtime: AgentRuntime) -> Option<UnixMs> {
        None
    }
}

/// The main checkout of the repository a linked worktree belongs to.
fn repository_root(runner: &mut SystemRunner, checkout: &Path) -> Result<PathBuf, Failure> {
    let output = hide_factory::exec::checked(
        runner,
        "worktree.root",
        "git",
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        Some(checkout),
    )?;
    let common = PathBuf::from(output.trim());
    common
        .parent()
        .filter(|_| common.file_name().is_some_and(|name| name == ".git"))
        .map(Path::to_path_buf)
        .ok_or_else(|| Failure::task("worktree.root", "the repository has no main checkout"))
}

fn spawn_failure(reason: &str) -> Failure {
    match reason {
        // The pane was made and the agent typed in; its session is not
        // visible yet. The same intent continues the spawn later.
        "native_identity_unavailable" => Failure::starting("worker.spawn", reason),
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

struct MachineEnvironment;

impl Environment for MachineEnvironment {
    fn disk_free(&mut self, project: &str) -> Option<u64> {
        hide_platform::fs::space::free_bytes(Path::new(project)).ok()
    }

    /// macOS reports 1 (normal), 2 (warn) or 4 (critical).
    fn memory_pressure(&mut self) -> MemoryPressure {
        if !cfg!(target_os = "macos") {
            return MemoryPressure::Normal;
        }
        let mut runner = SystemRunner {
            stop: Arc::new(AtomicBool::new(false)),
        };
        match hide_factory::exec::checked(
            &mut runner,
            "memory",
            "/usr/sbin/sysctl",
            &["-n", "kern.memorystatus_vm_pressure_level"],
            None,
        )
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        {
            Some(4) => MemoryPressure::Critical,
            Some(2) => MemoryPressure::Warn,
            _ => MemoryPressure::Normal,
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
        let connector = guard(&runtime).delivery_connector("local");
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
        let settings = lock(&runtime)
            .and_then(|runtime| guard(&runtime).factory_ai_settings())
            .or_else(|| {
                home.as_deref()
                    .and_then(|home| hide_ai::settings::load(home).ok())
            })
            .unwrap_or_default();
        if router
            .as_ref()
            .is_none_or(|(current, _)| *current != settings)
        {
            router = Some((settings.clone(), crate::ai::factory_router(&settings)));
        }
        let Some((_, router)) = &router else { continue };
        let request = hide_ai::AiRequest {
            feature_id: judgment.feature_id(),
            request_id: hide_ai::RequestId(judgment.id.clone()),
            subject_id: judgment
                .task
                .clone()
                .unwrap_or_else(|| judgment.factory.clone()),
            system: judgment.system().to_owned(),
            input: judgment.render_input(),
            output_schema: judgment.schema(),
            deadline: JUDGMENT_DEADLINE,
            schema_version: hide_factory::judgment::SCHEMA_VERSION,
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

    fn git(cwd: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn a_worker_worktree_outside_its_repository_resolves_to_the_main_checkout() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("project");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "--quiet", "-b", "main"]);
        git(
            &main,
            &[
                "-c",
                "user.email=f@example.com",
                "-c",
                "user.name=F",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "base",
            ],
        );
        // Spawned worktrees live in a folder of their own, not in the repository.
        let worktree = root.path().join("worktrees/project/factory-l1-task");
        git(
            &main,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "factory/1-task",
                worktree.to_str().unwrap(),
            ],
        );
        let mut runner = SystemRunner {
            stop: Arc::new(AtomicBool::new(false)),
        };
        let found = repository_root(&mut runner, &worktree).unwrap();
        assert_eq!(found.canonicalize().unwrap(), main.canonicalize().unwrap());
        hide_host::worktrees::remove_worktree(&found, &worktree, true).unwrap();
        assert!(!worktree.exists());
    }
}
