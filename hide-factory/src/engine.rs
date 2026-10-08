//! The engine: every Factory on this machine, their Tasks and the rules that
//! move them (D-01, D-28, D-29). It is one value driven from one thread:
//! `command` for a caller, `letter` for mail addressed to a Factory, and
//! `tick` for time and the outside world. Each change is saved before the
//! answer leaves (B72).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};

use crate::adapters::{
    Clock, EnvSignal, Environment, Failure, Judge, MainCheck, MemoryPressure, MergeTarget,
    Notifier, OutsideEvent, PreMerge, Removal, RepoContext, RevertRef, TaskSource, Verifier,
    VerifyPoll, VerifyRun, WorkerRuntime, WorkerSpawn, WorkerStatus,
};
use crate::command::{CardInput, Command, Refusal, VerificationChoice};
use crate::dag;
use crate::judgment::{self, Judgment, JudgmentInput, JudgmentOutcome, OtherTask, Priority};
use crate::model::*;
use crate::role::{Permission, ROLE_NOT_ALLOWED, Role};
use crate::store::{Event, Record, Store, StoreError, sha256_hex};
use crate::summary::{self, FactorySummary};

mod observer;

/// The world the engine acts on.
pub struct Ports {
    pub clock: Box<dyn Clock + Send>,
    pub source: Box<dyn TaskSource + Send>,
    pub verifier: Box<dyn Verifier + Send>,
    pub merge: Box<dyn MergeTarget + Send>,
    pub workers: Box<dyn WorkerRuntime + Send>,
    pub judge: Box<dyn Judge + Send>,
    pub environment: Box<dyn Environment + Send>,
    pub notifier: Box<dyn Notifier + Send>,
}

/// Mail addressed to a Factory's code-owned recipient (D-14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inbound {
    pub id: String,
    pub factory: String,
    pub sender_pane: String,
    /// `request`, `block`, `report` or `watch`.
    pub kind: String,
    pub body: String,
}

pub const TITLE_LIMIT: usize = 200;
pub const TEXT_LIMIT: usize = 4_000;
pub const LIST_LIMIT: usize = 30;
pub const TASK_LIMIT: usize = 5_000;
/// Wait between retries of a rate-limited or failing GitHub read (B58).
const BACKOFF_MS: [u64; 5] = [
    MINUTE_MS,
    2 * MINUTE_MS,
    5 * MINUTE_MS,
    15 * MINUTE_MS,
    30 * MINUTE_MS,
];
const PROCESSED_LETTERS: usize = 4_096;
const CASCADE_WINDOW_MS: u64 = 30 * MINUTE_MS;
const ENV_RECHECK_MS: u64 = MINUTE_MS;
/// How often a worker whose agent has not shown a session is asked again,
/// and when the person is told to look at its pane.
const START_RETRY_MS: u64 = 30_000;
/// The most questions, decisions and discoveries one Task keeps; a worker
/// report past it is refused.
pub const REPORT_LIMIT: usize = 500;
/// How often a broken main, or a main head with checks still running, is
/// read again.
const MAIN_CHECK_EVERY_MS: u64 = 30_000;
/// When a merge that GitHub answered without its commit is asked again.
pub const MERGE_COMMIT_AGAIN_MS: u64 = 30_000;
/// What a GitHub Factory reads, as `init` names it before a person confirms.
pub const GITHUB_READS: [&str; 5] = [
    "issues labelled factory",
    "pull requests",
    "check runs and commit statuses",
    "workflow runs",
    "the default branch's required checks",
];
/// What a GitHub Factory writes, as `init` names it before a person confirms.
pub const GITHUB_WRITES: [&str; 6] = [
    "the factory label",
    "issue create and edit",
    "branch push to factory/*",
    "pull request open, merge, close and reopen",
    "revert pull request",
    "rerun of a failed workflow run",
];
/// How long a merge GitHub answered without naming its commit is read again
/// before a person looks.
pub const MERGE_UNNAMED_LIMIT_MS: u64 = 10 * 60_000;
/// When a failed push or pull request of a reported Task is tried again.
const PUBLISH_RETRY_MS: u64 = 60_000;
/// A GitHub Task's report whose commits are not pushed yet.
const PUBLISH_PENDING: &str = "publish_pending";
/// A reported Task whose checks wait for its paused Factory to resume (D-48).
const CHECKS_DEFERRED: &str = "checks_deferred";
const START_NOTICE_MS: u64 = 10 * MINUTE_MS;

struct VerifyState {
    run: VerifyRun,
    stage: AttemptStage,
}

/// Failed store writes: a running count, and the ones the host has not
/// logged yet (at most [`STORE_FAILURE_LIMIT`]).
#[derive(Default)]
struct StoreFailures {
    total: u64,
    unlogged: Vec<StoreFailure>,
}

/// One failed store write, for the diagnostic log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreFailure {
    pub factory: String,
    pub task: Option<String>,
    pub stage: String,
    pub error: String,
}

const STORE_FAILURE_LIMIT: usize = 64;

#[derive(Clone)]
enum RevertPhase {
    /// A skipped or cancelled run was asked again; waits for its result.
    Bisecting {
        index: usize,
    },
    Reverting {
        revert: RevertRef,
        task: String,
    },
}

/// A failure the cascade rules read (D-31 rule 3).
struct FailureNote {
    at: UnixMs,
    factory: String,
    task: String,
    stage: String,
    kind: FailureKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FailureKind {
    /// Read as the environment's (a structured signal).
    Environment,
    /// Counted against the Task (a verification failure).
    Task,
}

impl FailureKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Environment => "environment",
            Self::Task => "task",
        }
    }
}

pub struct Engine {
    store: Store,
    ports: Ports,
    factories: BTreeMap<String, Factory>,
    tasks: BTreeMap<String, BTreeMap<String, Task>>,
    verifying: BTreeMap<(String, String), VerifyState>,
    /// Judgments waiting: id -> (factory, task, purpose).
    judgments: BTreeMap<String, (String, Option<String>, Purpose)>,
    /// Drift and user checks still running per Task.
    checks_running: BTreeMap<(String, String), u32>,
    reverts: BTreeMap<String, RevertPhase>,
    /// Main heads already checked per Factory.
    main_seen: BTreeMap<String, String>,
    /// Store writes that failed: the host logs the new ones and `status`
    /// shows the count, since an event about a failed write may fail too.
    store_failures: RefCell<StoreFailures>,
    /// Factories whose main head has a check still running: read again at
    /// the paced interval until it finishes (B47).
    main_pending: BTreeSet<String>,
    /// When each Factory's main was last read, to pace a broken or pending
    /// main to [`MAIN_CHECK_EVERY_MS`].
    main_checked_at: BTreeMap<String, UnixMs>,
    /// When a failed push or pull request is tried again, per Task.
    publish_retry: BTreeMap<(String, String), UnixMs>,
    /// Refusals in a row of a Task's push or pull request that no
    /// environment signal explains.
    publish_refusals: BTreeMap<(String, String), u32>,
    /// When a merge asked to wait is tried again, per Task, and since when
    /// the merge may have landed with its commit not named yet.
    merge_retry: BTreeMap<(String, String), (UnixMs, Option<UnixMs>)>,
    processed: BTreeSet<String>,
    processed_order: Vec<String>,
    /// Recent environment and Task failures the cascade rules read.
    env_failures: Vec<FailureNote>,
    /// New starts halted by a cascade until this time (B60).
    halt_until: Option<UnixMs>,
    env_problem_since: Option<UnixMs>,
    env_diagnosed: bool,
    hold_checked_at: Option<UnixMs>,
    hold_reason: Option<EnvHold>,
    /// A runtime unavailable until its usage resets (B58).
    runtime_blocked: BTreeMap<Runtime, UnixMs>,
    github_backoff: BTreeMap<String, (u32, UnixMs)>,
    /// The `hide` program a worker runs: the one beside this daemon, so a
    /// worker never reaches an older copy earlier on its PATH.
    hide_program: String,
    /// Workers whose agent has not shown a session yet: (first seen, next
    /// ask). Each holds its slot (B57's count) until it starts or is given up.
    starting: BTreeMap<(String, String), (UnixMs, UnixMs)>,
    machine_max_workers: u32,
    question_seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Purpose {
    Intake,
    Drift,
    Check,
    /// A periodic user check on a running Task: it adds questions or marks
    /// and never holds a merge (B67).
    Periodic,
    /// The board the watch read, as of this time.
    Watch {
        read_at: UnixMs,
    },
    Env,
    /// The Observer sorting one decision request (D-14).
    Classify {
        question: String,
    },
    /// The Observer reading a worker resting without a report (D-23).
    Diagnose,
    /// The Observer deciding a risk-path merge (D-21).
    RiskMerge,
}

type Reply = Result<Value, Refusal>;

fn refuse(reason: &str, next: impl Into<String>) -> Refusal {
    Refusal::new(reason, next)
}

impl Engine {
    pub fn set_hide_program(&mut self, program: impl Into<String>) {
        self.hide_program = program.into();
    }

    pub fn open(store: &Path, files: &Path, ports: Ports) -> Result<Self, StoreError> {
        let store = Store::open(store, files)?;
        let loaded = store.load()?;
        let mut engine = Self {
            store,
            ports,
            factories: BTreeMap::new(),
            tasks: BTreeMap::new(),
            verifying: BTreeMap::new(),
            judgments: BTreeMap::new(),
            checks_running: BTreeMap::new(),
            reverts: BTreeMap::new(),
            main_seen: BTreeMap::new(),
            store_failures: RefCell::new(StoreFailures::default()),
            main_pending: BTreeSet::new(),
            main_checked_at: BTreeMap::new(),
            publish_retry: BTreeMap::new(),
            publish_refusals: BTreeMap::new(),
            merge_retry: BTreeMap::new(),
            processed: BTreeSet::new(),
            processed_order: Vec::new(),
            env_failures: Vec::new(),
            halt_until: None,
            env_problem_since: None,
            env_diagnosed: false,
            hold_checked_at: None,
            hold_reason: None,
            runtime_blocked: BTreeMap::new(),
            github_backoff: BTreeMap::new(),
            hide_program: "hide".into(),
            starting: BTreeMap::new(),
            machine_max_workers: 5,
            question_seq: 0,
        };
        for factory in loaded.factories {
            engine.tasks.entry(factory.id.clone()).or_default();
            engine.factories.insert(factory.id.clone(), factory);
        }
        for task in loaded.tasks {
            for question in &task.questions {
                if let Some(number) = question
                    .id
                    .strip_prefix('Q')
                    .and_then(|n| n.parse::<u64>().ok())
                {
                    engine.question_seq = engine.question_seq.max(number);
                }
            }
            engine
                .tasks
                .entry(task.factory.clone())
                .or_default()
                .insert(task.id.clone(), task);
        }
        for (key, value) in loaded.meta {
            match key.as_str() {
                "machine.max_workers" => {
                    engine.machine_max_workers = value.parse().unwrap_or(5);
                }
                "processed_letters" => {
                    let ids: Vec<String> = serde_json::from_str(&value).unwrap_or_default();
                    engine.processed_order = ids.clone();
                    engine.processed = ids.into_iter().collect();
                }
                _ => {}
            }
        }
        // A restart resumes what was in flight (B72): verifications start
        // again from their recorded attempt, since a child of the old
        // process is gone; reviews still pending are asked again.
        let now = engine.now();
        let pending: Vec<(String, String, TaskState, ReviewState)> = engine
            .all_tasks()
            .map(|task| {
                (
                    task.factory.clone(),
                    task.id.clone(),
                    task.state,
                    task.review.clone(),
                )
            })
            .collect();
        for (factory, id, state, review) in pending {
            if state == TaskState::Verifying {
                engine.restart_verification(&factory, &id);
            }
            if state == TaskState::Drafting && matches!(review, ReviewState::Requested { .. }) {
                engine.request_review(&factory, &id, now);
            }
        }
        // An Observer judgment in flight died with the old process: its
        // request goes to a person, and a diagnosis counts as failed (B10).
        engine.settle_lost_observer_calls();
        // A main recovery cut by the restart is not guessed again: which
        // merge it was reverting is gone, so a person picks the next step.
        let cut: Vec<(String, Vec<LandedMerge>)> = engine
            .factories
            .values()
            .filter(|f| f.main.broken && f.main.recovering && !f.main.needs_person)
            .map(|f| (f.id.clone(), f.main.merges_since_green.clone()))
            .collect();
        for (factory, merges) in cut {
            engine.revert_needs_person(&factory, "hided가 다시 시작해 복구가 끊겼습니다", &merges);
        }
        Ok(engine)
    }

    fn now(&self) -> UnixMs {
        self.ports.clock.now()
    }

    fn all_tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks.values().flat_map(BTreeMap::values)
    }

    pub fn factories(&self) -> impl Iterator<Item = &Factory> {
        self.factories.values()
    }

    pub fn task(&self, factory: &str, id: &str) -> Option<&Task> {
        self.tasks.get(factory)?.get(id)
    }

    pub fn tasks_of(&self, factory: &str) -> impl Iterator<Item = &Task> {
        self.tasks
            .get(factory)
            .into_iter()
            .flat_map(BTreeMap::values)
    }

    pub fn summary(&self) -> FactorySummary {
        let factories: Vec<&Factory> = self.factories.values().collect();
        let tasks: Vec<&Task> = self.all_tasks().collect();
        summary::build(
            &factories,
            &tasks,
            self.now(),
            self.ports.clock.utc_offset_ms(),
            &self.runtime_blocked,
        )
    }

    pub fn events(&self, factory: &str, task: Option<&str>, limit: usize) -> Vec<Event> {
        self.store.events(factory, task, limit).unwrap_or_default()
    }

    /// The role of a caller as the host saw it (D-33). Everything that names
    /// a worker binds to it: the caller's own pane or folder, a pane it
    /// claimed, or an agent above it in the spawn lineage, by its id or by
    /// the pane it was registered on. A lineage cut short may hide a worker
    /// above, so such a caller only reads, never acts as an operator.
    pub fn caller_role(&self, caller: &Caller<'_>, command: &Command) -> Result<Role, Refusal> {
        let bound = self
            .role_for(caller.pane, caller.cwd)
            .or_else(|| {
                caller
                    .claimed
                    .and_then(|pane| self.role_for(Some(pane), None))
            })
            .or_else(|| self.role_for_agents(caller.ancestor_agents))
            .or_else(|| {
                caller
                    .ancestor_panes
                    .iter()
                    .find_map(|pane| self.role_for(Some(pane), None))
            });
        match bound {
            Some((factory, task)) => Ok(Role::Worker { factory, task }),
            None if !caller.lineage_complete && command.permission() != Permission::Read => {
                Err(Refusal::new(
                    "lineage_unknown",
                    "The caller's spawn lineage could not be read to its root; run this from an operator pane",
                ))
            }
            // An agent a Factory started that is no Task's worker now (a
            // start abandoned on its way) is never an operator.
            None if caller.factory_spawned && command.permission() != Permission::Read => {
                Err(Refusal::new(
                    "factory_agent_unbound",
                    "This agent was started by a Factory for a Task it no longer holds; run this from an operator pane",
                ))
            }
            None => Ok(Role::Operator {
                pane: caller.pane.unwrap_or("checkout").to_owned(),
            }),
        }
    }

    /// The Task whose worker is one of `agents` (a caller's spawn lineage):
    /// a worker's child acts as that worker (D-33).
    pub fn role_for_agents(&self, agents: &[String]) -> Option<(String, String)> {
        let bound = |task: &&Task| {
            !task.purged
                && task
                    .worker
                    .as_ref()
                    .and_then(|worker| worker.agent.as_ref())
                    .is_some_and(|agent| agents.contains(agent))
        };
        let finished = |task: &&Task| matches!(task.state, TaskState::Cancelled | TaskState::Done);
        let live = self.all_tasks().filter(|t| !finished(t)).find(&bound);
        live.or_else(|| self.all_tasks().filter(&finished).find(&bound))
            .map(|task| (task.factory.clone(), task.id.clone()))
    }

    /// The role of a caller (D-33): a pane the Factory spawned, or a cwd
    /// inside a Factory worktree, is a worker of that Task. A finished or
    /// cancelled Task's worker stays a worker while its worktree and pane
    /// stay, so its pane never gains the operator's commands; a live Task
    /// wins when a pane or folder was reused. A purged Task binds nothing,
    /// because Herdr can give its closed pane id to the operator's next pane.
    pub fn role_for(&self, pane: Option<&str>, cwd: Option<&str>) -> Option<(String, String)> {
        let bound = |task: &&Task| {
            let Some(worker) = task.worker.as_ref().filter(|_| !task.purged) else {
                return false;
            };
            if pane.is_some() && worker.pane.as_deref() == pane {
                return true;
            }
            !worker.worktree.is_empty()
                && cwd.is_some_and(|cwd| Path::new(cwd).starts_with(&worker.worktree))
        };
        let finished = |task: &&Task| matches!(task.state, TaskState::Cancelled | TaskState::Done);
        let live = self.all_tasks().filter(|t| !finished(t)).find(&bound);
        live.or_else(|| self.all_tasks().filter(&finished).find(&bound))
            .map(|task| (task.factory.clone(), task.id.clone()))
    }

    /// The one current Task-held spawn on a pane. This is deliberately
    /// narrower than command roles: neither cwd nor descendants lend identity.
    /// The caller must independently prove this spawn's current native session.
    pub fn question_worker(&self, pane: &str) -> Option<&WorkerRef> {
        let mut workers = self.all_tasks().filter_map(|task| {
            let worker = task.worker.as_ref()?;
            (!task.purged
                && worker.factory == task.factory
                && worker.pane.as_deref() == Some(pane)
                && worker.agent.as_deref().is_some_and(|id| !id.is_empty()))
            .then_some(worker)
        });
        let worker = workers.next()?;
        workers.next().is_none().then_some(worker)
    }

    pub fn factory_for_project(&self, project: &str) -> Option<&Factory> {
        let path = Path::new(project);
        self.factories
            .values()
            .filter(|factory| path.starts_with(&factory.project))
            .max_by_key(|factory| factory.project.len())
    }

    fn record(&self, factory: &str, task: Option<&str>, kind: &str, detail: Value) {
        let event = Event {
            factory: factory.to_owned(),
            task: task.map(str::to_owned),
            at: self.now(),
            kind: kind.to_owned(),
            detail,
        };
        // A failed event write never changes a decision; it is counted and
        // reaches the diagnostic log through the host.
        if let Err(error) = self.store.append_event(&event) {
            self.store_failed(factory, task, "event", &error.0);
        }
    }

    fn store_failed(&self, factory: &str, task: Option<&str>, stage: &str, error: &str) {
        let mut failures = self.store_failures.borrow_mut();
        failures.total += 1;
        if failures.unlogged.len() < STORE_FAILURE_LIMIT {
            failures.unlogged.push(StoreFailure {
                factory: factory.to_owned(),
                task: task.map(str::to_owned),
                stage: stage.to_owned(),
                error: judgment::cut(error, 300),
            });
        }
    }

    /// Failed store writes since the last call, for the diagnostic log.
    pub fn take_store_failures(&self) -> Vec<StoreFailure> {
        std::mem::take(&mut self.store_failures.borrow_mut().unlogged)
    }

    /// Keeps a judgment or letter body with its Task (D-58); like an event,
    /// a failed write never changes a decision.
    fn keep(&self, factory: &str, task: Option<&str>, kind: &str, reference: &str, body: &str) {
        let record = Record {
            factory: factory.to_owned(),
            task: task.map(str::to_owned),
            at: self.now(),
            kind: kind.to_owned(),
            reference: reference.to_owned(),
            body: body.to_owned(),
        };
        if let Err(error) = self.store.keep_record(&record) {
            self.store_failed(factory, task, "record", &error.0);
            self.record(
                factory,
                task,
                "store.failed",
                json!({"stage": "record", "error": error.0}),
            );
        }
    }

    /// Every judgment's input is kept before it is queued (D-58).
    fn submit_judgment(&mut self, mut judgment: Judgment) -> Result<(), Failure> {
        // A paused Factory asks its AI nothing (D-48).
        let Some(factory) = self.factories.get(&judgment.factory) else {
            return Err(Failure::task("judgment", "unknown Factory"));
        };
        if factory.paused {
            return Err(Failure::task("judgment", "paused"));
        }
        judgment.ai = factory.config.factory_ai.clone();
        self.keep(
            &judgment.factory,
            judgment.task.as_deref(),
            "judgment.input",
            &judgment.id,
            &judgment.render_input(),
        );
        self.ports.judge.submit(judgment)
    }

    fn save_factory(&mut self, id: &str) {
        if let Some(factory) = self.factories.get(id)
            && let Err(error) = self.store.put_factory(factory)
        {
            self.store_failed(id, None, "factory", &error.0);
            self.record(
                id,
                None,
                "store.failed",
                json!({"stage": "factory", "error": error.0}),
            );
        }
    }

    fn save(&mut self, factory: &str, id: &str) {
        let Some(task) = self
            .tasks
            .get(factory)
            .and_then(|tasks| tasks.get(id))
            .cloned()
        else {
            return;
        };
        if let Err(error) = self.store.put_task(&task) {
            self.store_failed(factory, Some(id), "task", &error.0);
            self.record(
                factory,
                Some(id),
                "store.failed",
                json!({"stage": "task", "error": error.0}),
            );
        }
    }

    fn with_task<R>(
        &mut self,
        factory: &str,
        id: &str,
        change: impl FnOnce(&mut Task) -> R,
    ) -> Option<R> {
        let now = self.now();
        let task = self.tasks.get_mut(factory)?.get_mut(id)?;
        let result = change(task);
        task.updated_at = now;
        self.save(factory, id);
        Some(result)
    }

    fn set_state(&mut self, factory: &str, id: &str, state: TaskState) {
        let now = self.now();
        let mut from = None;
        self.with_task(factory, id, |task| {
            if task.state != state {
                from = Some(task.state);
                task.state = state;
                task.state_since = now;
                if state == TaskState::Done {
                    task.done_at = Some(now);
                    task.seen = false;
                }
                if state != TaskState::Stopped {
                    task.stop = None;
                    task.stop_detail = None;
                    task.diagnosis = None;
                }
                if state != TaskState::Paused {
                    task.pause_reason = None;
                }
            }
        });
        if let Some(from) = from {
            // A merge or push asked again belongs to the stay it was asked in.
            let key = (factory.to_owned(), id.to_owned());
            self.merge_retry.remove(&key);
            self.publish_refusals.remove(&key);
            self.record(
                factory,
                Some(id),
                "task.state",
                json!({"from": from.as_str(), "to": state.as_str()}),
            );
        }
    }

    fn next_question_id(&mut self) -> String {
        self.question_seq += 1;
        format!("Q{}", self.question_seq)
    }

    #[allow(clippy::too_many_arguments)]
    fn add_question(
        &mut self,
        factory: &str,
        id: &str,
        origin: QuestionOrigin,
        kind: QuestionKind,
        text: &str,
        suggestion: &str,
        default_action: Option<String>,
        deadline: Option<UnixMs>,
        choices: Vec<String>,
        letter: Option<String>,
    ) -> String {
        let question_id = self.next_question_id();
        let now = self.now();
        let question = Question {
            id: question_id.clone(),
            origin,
            kind,
            text: judgment::cut(text, TEXT_LIMIT),
            suggestion: judgment::cut(suggestion, TEXT_LIMIT),
            default_action,
            deadline,
            asked_at: now,
            choices,
            answer: None,
            letter,
            routing: None,
            notice: None,
            refers_to: None,
        };
        self.with_task(factory, id, |task| task.questions.push(question));
        self.record(
            factory,
            Some(id),
            "question.added",
            json!({"question": question_id}),
        );
        question_id
    }

    fn notice(&mut self, factory: &str, id: &str, text: &str) {
        self.add_question(
            factory,
            id,
            QuestionOrigin::Engine,
            QuestionKind::Notice,
            text,
            "",
            None,
            None,
            vec!["ok".into()],
            None,
        );
    }

    fn running_count(&self) -> u32 {
        self.all_tasks()
            .filter(|task| task.state.holds_slot())
            .count() as u32
            + self.starting.len() as u32
    }

    // ----------------------------------------------------------------- commands

    pub fn command(&mut self, role: &Role, command: Command) -> Value {
        if !command.permission().allowed(role) {
            return Refusal::new(
                ROLE_NOT_ALLOWED,
                "This command needs a person; ask in an operator pane",
            )
            .with(json!({"role": role.name(), "verb": command.verb()}))
            .to_json();
        }
        let verb = command.verb();
        // A worker's reports grow its Task record: past the cap a report is
        // refused, never stored (rule 15). `done` still finishes the Task, and
        // `block` still stops it for a person; each needs a running Task.
        if command.permission() == Permission::Report
            && !matches!(command, Command::Done { .. } | Command::Block { .. })
            && let Role::Worker { factory, task } = role
            && let Some(task) = self.task(factory, task)
            && task.questions.len() + task.decisions.len() + task.discoveries.len() >= REPORT_LIMIT
        {
            return Refusal::new(
                "report_limit",
                "This Task holds too many questions and decisions; finish with hide factory done",
            )
            .with(json!({"limit": REPORT_LIMIT}))
            .to_json();
        }
        let answer = self.run_command(role, command);
        match answer {
            Ok(mut value) => {
                if value.get("ok").is_none() {
                    value["ok"] = json!(true);
                }
                value
            }
            Err(refusal) => {
                self.record(
                    "-",
                    None,
                    "command.refused",
                    json!({"verb": verb, "reason": refusal.reason}),
                );
                refusal.to_json()
            }
        }
    }

    fn run_command(&mut self, role: &Role, command: Command) -> Reply {
        match command {
            Command::Init {
                project,
                verification,
                merge_mode,
                confirm,
            } => self.init(&project, verification, merge_mode, confirm),
            Command::Add {
                project,
                task,
                issue,
                card,
                producer_pane,
            } => self.add(
                role,
                project.as_deref(),
                task.as_deref(),
                issue.as_deref(),
                card,
                producer_pane,
            ),
            Command::Status { project } => self.status(project.as_deref()),
            Command::Show { task } => {
                let (factory, id) = self.resolve(role, &task)?;
                // A person opening a finished Task has seen it (D-30).
                if matches!(role, Role::Operator { .. })
                    && self
                        .task(&factory, &id)
                        .is_some_and(|t| t.state == TaskState::Done && !t.seen)
                {
                    self.with_task(&factory, &id, |task| task.seen = true);
                }
                Ok(json!({"task": self.show(&factory, &id)}))
            }
            Command::Inbox => {
                let summary = self.summary();
                Ok(
                    json!({"count": summary.my_turn, "notices": summary.notices, "items": summary.inbox}),
                )
            }
            Command::Answer {
                task,
                question,
                choice,
                text,
                change,
            } => {
                let (factory, id) = self.resolve(role, &task)?;
                self.answer(
                    role,
                    &factory,
                    &id,
                    question.as_deref(),
                    choice,
                    text,
                    change,
                )
            }
            Command::Ask {
                text,
                suggestion,
                default_action,
                deadline_hours,
                letter,
                choices,
            } => {
                let (factory, id) = self.own_task(role)?;
                self.ask(
                    &factory,
                    &id,
                    &text,
                    &suggestion,
                    Some(default_action),
                    deadline_hours,
                    letter,
                    choices,
                )
            }
            Command::Block {
                text,
                suggestion,
                deadline_hours,
                letter,
                choices,
            } => {
                let (factory, id) = self.own_task(role)?;
                self.ask(
                    &factory,
                    &id,
                    &text,
                    &suggestion,
                    None,
                    deadline_hours,
                    letter,
                    choices,
                )
            }
            Command::Propose {
                class,
                text,
                card,
                autonomy,
                reclassify,
                letter,
            } => {
                let (factory, id) = self.own_task(role)?;
                self.propose(
                    &factory, &id, class, &text, card, autonomy, reclassify, letter,
                )
            }
            Command::Done {
                summary,
                breaking,
                letter,
            } => {
                let (factory, id) = self.own_task(role)?;
                self.done(&factory, &id, summary, breaking, letter)
            }
            Command::Decide { text } => {
                let (factory, id) = self.own_task(role)?;
                let by = role.relayed_by();
                let now = self.now();
                self.with_task(&factory, &id, |task| {
                    task.decisions.push(DecisionRecord {
                        text: judgment::cut(&text, TEXT_LIMIT),
                        by,
                        at: now,
                        kind: None,
                        reason: None,
                    })
                });
                Ok(json!({"message": "decision recorded"}))
            }
            Command::Config { project, set } => self.config(role, project.as_deref(), set),
            Command::Priority { task, priority } => {
                let (factory, id) = self.resolve(role, &task)?;
                self.allowed(&factory, &id, "priority")?;
                self.with_task(&factory, &id, |task| task.human.priority = priority);
                Ok(self.task_answer(&factory, &id, "priority set"))
            }
            Command::Dep { task, on, remove } => self.dep(role, &task, &on, remove),
            Command::Pause { task } => {
                let (factory, id) = self.resolve(role, &task)?;
                self.allowed(&factory, &id, "pause")?;
                self.put_to_sleep(&factory, &id);
                self.set_state(&factory, &id, TaskState::Paused);
                self.with_task(&factory, &id, |task| {
                    task.pause_reason = Some(PauseReason::Person)
                });
                Ok(self.task_answer(&factory, &id, "paused; slot released"))
            }
            Command::Resume { task } => {
                let (factory, id) = self.resolve(role, &task)?;
                self.allowed(&factory, &id, "resume")?;
                // A person starting it again gives back the automatic restart (D-25).
                self.with_task(&factory, &id, |task| task.auto_restarts = 0);
                self.set_state(&factory, &id, TaskState::Waiting);
                Ok(self.task_answer(&factory, &id, "resumes when a slot is free"))
            }
            Command::PauseFactory { project } => {
                let factory = self.factory_id(project.as_deref())?;
                self.pause_factory(&factory);
                Ok(
                    json!({"message": "paused: no starts, AI judgments or auto merges; workers asleep"}),
                )
            }
            Command::ResumeFactory { project } => {
                let factory = self.factory_id(project.as_deref())?;
                self.resume_factory(&factory);
                Ok(json!({"message": "resumed: workers continue in their worktrees"}))
            }
            Command::AckNotices { project } => {
                // "모두 확인" is one action: without a project it clears
                // every open Factory's notices (D-43).
                let factories: Vec<String> = match project.as_deref() {
                    Some(project) => vec![self.factory_id(Some(project))?],
                    None => self
                        .factories
                        .values()
                        .filter(|f| !f.closed)
                        .map(|f| f.id.clone())
                        .collect(),
                };
                if factories.is_empty() {
                    return Err(refuse(
                        "factory_not_found",
                        "Create one with hide factory init <project>",
                    ));
                }
                let cleared: usize = factories
                    .iter()
                    .map(|factory| self.ack_notices(factory, role))
                    .sum();
                Ok(json!({"message": "notices cleared", "cleared": cleared}))
            }
            Command::Worker { task, worker } => {
                let (factory, id) = self.resolve(role, &task)?;
                self.pin_worker(&factory, &id, worker)
            }
            Command::Retry { task } => {
                let (factory, id) = self.resolve(role, &task)?;
                self.allowed(&factory, &id, "retry")?;
                self.with_task(&factory, &id, |task| {
                    task.failures = 0;
                    task.environment_failures = 0;
                    task.auto_restarts = 0;
                    task.recovery = None;
                    task.diagnosis = None;
                    for question in task.questions.iter_mut().filter(|q| q.open()) {
                        if matches!(question.kind, QuestionKind::Action) {
                            question.answer = Some(Answer {
                                text: "retry".into(),
                                chose: Some("retry".into()),
                                relayed_by: role.relayed_by(),
                                at: 0,
                            });
                        }
                    }
                });
                self.set_state(&factory, &id, TaskState::Waiting);
                Ok(self.task_answer(
                    &factory,
                    &id,
                    "restarts in the same worktree when a slot is free",
                ))
            }
            Command::Merge { task } => {
                let (factory, id) = self.resolve(role, &task)?;
                self.allowed(&factory, &id, "merge")?;
                self.manual_merge(role, &factory, &id)
            }
            Command::RequestChanges { task, comment } => {
                let (factory, id) = self.resolve(role, &task)?;
                self.allowed(&factory, &id, "request-changes")?;
                if comment.trim().is_empty() {
                    return Err(refuse(
                        "comment_required",
                        "Add --comment with what to change",
                    ));
                }
                let by = role.relayed_by();
                let now = self.now();
                self.with_task(&factory, &id, |task| {
                    task.decisions.push(DecisionRecord {
                        text: format!("수정 요청: {}", judgment::cut(&comment, TEXT_LIMIT)),
                        by,
                        at: now,
                        kind: None,
                        reason: None,
                    });
                    task.gates.clear();
                });
                self.set_state(&factory, &id, TaskState::Running);
                self.wake(&factory, &id, &format!(
                    "Factory: 사람이 수정을 요청했습니다.\n{comment}\n고친 뒤 다시 hide factory done 하세요."
                ));
                Ok(self.task_answer(&factory, &id, "sent back to the worker"))
            }
            Command::Cancel { task } => {
                let (factory, id) = self.resolve(role, &task)?;
                self.allowed(&factory, &id, "cancel")?;
                self.cancel(&factory, &id);
                Ok(self.task_answer(&factory, &id, "cancelled; revivable for 7 days"))
            }
            Command::Revive { task } => {
                let (factory, id) = self.resolve(role, &task)?;
                // A cancelled Task past its keep period says so (B71).
                if self
                    .task(&factory, &id)
                    .is_none_or(|t| t.state != TaskState::Cancelled)
                {
                    self.allowed(&factory, &id, "revive")?;
                }
                self.revive(&factory, &id)
            }
            Command::Close { project } => self.close(role, project.as_deref()),
            Command::Check {
                project,
                at,
                instruction,
            } => {
                let factory = self.factory_id(project.as_deref())?;
                if instruction.trim().is_empty() {
                    return Err(refuse("instruction_required", "Give --instruction"));
                }
                if let Some(f) = self.factories.get_mut(&factory) {
                    f.config.checks.push(UserCheck {
                        at,
                        instruction: judgment::cut(&instruction, TEXT_LIMIT),
                    });
                }
                self.save_factory(&factory);
                Ok(json!({"message": "check added"}))
            }
        }
    }

    fn factory_id(&self, project: Option<&str>) -> Result<String, Refusal> {
        match project {
            Some(project) => self
                .factory_for_project(project)
                .map(|factory| factory.id.clone())
                .ok_or_else(|| {
                    refuse(
                        "factory_not_found",
                        "Create one with hide factory init <project>",
                    )
                }),
            None => {
                let open: Vec<_> = self.factories.values().filter(|f| !f.closed).collect();
                match open.as_slice() {
                    [only] => Ok(only.id.clone()),
                    [] => Err(refuse(
                        "factory_not_found",
                        "Create one with hide factory init <project>",
                    )),
                    _ => Err(refuse("factory_ambiguous", "Pass --project <path>")),
                }
            }
        }
    }

    /// A Task by its id (`T-3`), issue (`#412`, `L-12`) or `factory/T-3`.
    fn resolve(&self, role: &Role, reference: &str) -> Result<(String, String), Refusal> {
        let within = match role {
            Role::Worker { factory, .. } => Some(factory.as_str()),
            _ => None,
        };
        self.resolve_within(within, reference)
    }

    /// A Task reference, inside `factory` when one is given: a dependency or
    /// a watch warning names a Task of its own Factory.
    fn resolve_within(
        &self,
        factory: Option<&str>,
        reference: &str,
    ) -> Result<(String, String), Refusal> {
        let reference = reference.trim();
        let mut found = Vec::new();
        for task in self.all_tasks() {
            if task.id == reference
                || task
                    .issue
                    .as_ref()
                    .is_some_and(|issue| issue.display() == reference)
                || format!("{}/{}", task.factory, task.id) == reference
            {
                found.push((task.factory.clone(), task.id.clone()));
            }
        }
        if let Some(factory) = factory {
            found.retain(|(f, _)| f == factory);
        }
        match found.as_slice() {
            [one] => Ok(one.clone()),
            [] => Err(refuse(
                "task_not_found",
                "Check the Task id or issue number with hide factory status",
            )),
            _ => Err(refuse("task_ambiguous", "Use <factory>/<task id>")),
        }
    }

    fn own_task(&self, role: &Role) -> Result<(String, String), Refusal> {
        match role {
            Role::Worker { factory, task } => Ok((factory.clone(), task.clone())),
            _ => Err(refuse(
                ROLE_NOT_ALLOWED,
                "Only a Factory worker reports on its Task",
            )),
        }
    }

    /// Actions a person may take in each state (Q29, B53).
    pub fn allowed_actions(task: &Task) -> Vec<&'static str> {
        let mut actions: Vec<&'static str> = match task.state {
            TaskState::Drafting => vec!["edit", "cancel"],
            TaskState::Waiting => vec!["priority", "dep-remove", "cancel"],
            TaskState::Running => vec!["pause", "cancel"],
            TaskState::Paused => vec!["resume", "cancel"],
            // Q29: a blocked Task takes only an answer.
            TaskState::Blocked => vec![],
            TaskState::Verifying => vec!["cancel"],
            TaskState::MergeWaiting => vec!["merge", "request-changes", "cancel"],
            TaskState::Stopped => vec!["retry", "cancel"],
            TaskState::Landed | TaskState::Done | TaskState::Relanding => vec![],
            TaskState::Outside if !task.purged => vec!["revive"],
            TaskState::Cancelled if !task.purged => vec!["revive"],
            TaskState::Cancelled | TaskState::Outside => vec![],
        };
        if task.open_questions().next().is_some() {
            actions.insert(0, "answer");
        }
        actions
    }

    fn allowed(&self, factory: &str, id: &str, action: &str) -> Result<(), Refusal> {
        let task = self
            .task(factory, id)
            .ok_or_else(|| refuse("task_not_found", "Check hide factory status"))?;
        let allowed = Self::allowed_actions(task);
        if allowed.contains(&action) {
            return Ok(());
        }
        Err(refuse(
            "action_not_allowed_in_state",
            "Choose one of the allowed actions",
        )
        .with(json!({"state": task.state.label(), "allowed": allowed})))
    }

    fn task_answer(&self, factory: &str, id: &str, message: &str) -> Value {
        let task = self.task(factory, id);
        json!({
            "message": message,
            "task": task.map(|task| json!({
                "id": task.id,
                "display_id": task.display_id(),
                "state": task.state.label(),
                "state_id": task.state.as_str(),
            })),
        })
    }

    // --------------------------------------------------------------------- init

    fn init(
        &mut self,
        project: &str,
        verification: Option<VerificationChoice>,
        merge_mode: Option<MergeMode>,
        confirm: bool,
    ) -> Reply {
        let project = project.trim_end_matches('/').to_owned();
        if project.is_empty() || !Path::new(&project).is_absolute() {
            return Err(refuse(
                "project_required",
                "Give the project's absolute path",
            ));
        }
        if let Some(existing) = self.factories.values().find(|f| f.project == project) {
            if !existing.closed {
                return Ok(
                    json!({"existing": true, "factory": {"id": existing.id, "project": existing.project}}),
                );
            }
            if confirm {
                let id = existing.id.clone();
                if let Some(factory) = self.factories.get_mut(&id) {
                    factory.closed = false;
                }
                self.save_factory(&id);
                self.record(&id, None, "factory.reopened", json!({}));
                return Ok(
                    json!({"created": true, "reopened": true, "factory": {"id": id, "project": project}}),
                );
            }
        }
        let probe = match self.ports.source.probe(&project) {
            Ok(probe) => probe,
            Err(failure) => {
                let next = match failure.signal {
                    Some(EnvSignal::GithubAuth) => "Run gh auth login, then retry".to_owned(),
                    _ => "Check the project path and retry".to_owned(),
                };
                return Err(refuse("init_failed", &next).with(json!({"stage": failure.stage})));
            }
        };
        let source = if probe.github {
            SourceKind::Github
        } else {
            SourceKind::Local
        };
        let mut candidates = Vec::new();
        if probe.github {
            candidates.push(
                json!({"kind": "ci", "value": if probe.required_checks.is_empty() {
                    "(no required checks on the default branch)".to_owned()
                } else {
                    probe.required_checks.join(", ")
                }}),
            );
        }
        for command in &probe.verify_candidates {
            candidates.push(json!({"kind": "verify", "value": command}));
        }
        let verification = match verification {
            Some(VerificationChoice::Ci { checks }) => {
                if !probe.github {
                    return Err(refuse(
                        "ci_unavailable",
                        "A project without GitHub verifies with --verify <command>",
                    ));
                }
                // A check named nowhere would let any finished run decide:
                // the branch protection's required checks stand in, and with
                // none the person names them (D-21, D-53).
                let checks = if checks.is_empty() {
                    probe.required_checks.clone()
                } else {
                    checks
                };
                if checks.is_empty() {
                    return Err(refuse(
                        "ci_checks_required",
                        "The default branch requires no checks: name them with --ci <check,...>",
                    ));
                }
                Verification::Ci { checks }
            }
            Some(VerificationChoice::Commands { commands }) => {
                let commands: Vec<String> = commands
                    .into_iter()
                    .filter(|c| !c.trim().is_empty())
                    .collect();
                if commands.is_empty() {
                    return Err(refuse(
                        "verify_command_required",
                        "Give at least one --verify <command>",
                    ));
                }
                Verification::Commands { commands }
            }
            Some(VerificationChoice::None) => Verification::None,
            None => Verification::None,
        };
        let auto_unavailable =
            (!verification.configured()).then_some("검증이 없으면 auto를 쓸 수 없습니다");
        let requested_mode = merge_mode.unwrap_or(if verification.configured() {
            MergeMode::Auto
        } else {
            MergeMode::Manual
        });
        if requested_mode == MergeMode::Auto && !verification.configured() {
            return Err(refuse(
                "auto_needs_verification",
                "Choose --ci or --verify, or use --merge manual",
            )
            .with(json!({"candidates": candidates})));
        }
        // A GitHub Factory names the login it acts as, the repository and
        // everything it reads and writes there before a person confirms
        // (D-62); the create screen shows this same list.
        let github = match (probe.github, &probe.repo) {
            (false, _) => None,
            (true, Some(repo)) => {
                let Some(account) = probe.account.clone() else {
                    return Err(refuse(
                        "github_login_required",
                        "Run gh auth login, then retry",
                    ));
                };
                Some((account, repo.clone()))
            }
            (true, None) => {
                return Err(refuse(
                    "init_failed",
                    "GitHub did not name the repository; check the origin remote and retry",
                ));
            }
        };
        if !confirm {
            return Ok(json!({
                "preview": true,
                "project": project,
                "source": if probe.github { "github" } else { "local" },
                "candidates": candidates,
                "merge_mode": "auto",
                "auto_unavailable": auto_unavailable,
                "default_runtime": probe.runtimes.first().copied().unwrap_or(Runtime::CLAUDE),
                "github": github.as_ref().map(|(account, repo)| json!({
                    "account": account,
                    "repo": repo,
                    "reads": GITHUB_READS,
                    "writes": GITHUB_WRITES,
                })),
            }));
        }
        if !matches!(
            verification,
            Verification::Ci { .. } | Verification::Commands { .. } | Verification::None
        ) {
            unreachable!();
        }
        let id = format!("f-{}", &sha256_hex(project.as_bytes())[..10]);
        let now = self.now();
        let mut config = Config {
            verification,
            merge_mode: requested_mode,
            ..Config::default()
        };
        if let Some(method) = probe.merge_methods.first() {
            config.merge_method = *method;
        }
        // Installed Claude Code first, else Codex (D-45).
        if let Some(runtime) = probe.runtimes.first() {
            config.default_runtime = *runtime;
        }
        let factory = Factory {
            id: id.clone(),
            project: project.clone(),
            project_name: Path::new(&project)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| project.clone()),
            source,
            repo: probe.repo.clone(),
            default_branch: if probe.default_branch.is_empty() {
                "main".into()
            } else {
                probe.default_branch.clone()
            },
            config,
            closed: false,
            created_at: now,
            next_task: 1,
            next_local_issue: 1,
            main: MainHealth::default(),
            outside_read_at: None,
            outside_read_failures: 0,
            watch_day: 0,
            watch_sent_today: 0,
            watch_last_at: None,
            paused: false,
            observer_day: 0,
            observer_calls: 0,
            observer_cap_notice_day: 0,
            github_approval: github.map(|(account, repo)| GithubApproval {
                account,
                repo,
                at: now,
            }),
        };
        if let Err(failure) = self.ports.source.prepare(&factory) {
            return Err(self.github_refusal(&failure, "init"));
        }
        let approval = factory.github_approval.clone();
        self.factories.insert(id.clone(), factory);
        self.tasks.entry(id.clone()).or_default();
        self.save_factory(&id);
        self.record(
            &id,
            None,
            "factory.created",
            json!({"source": source_name(source)}),
        );
        if let Some(approval) = approval {
            self.record(
                &id,
                None,
                "github.approved",
                json!({"account": approval.account, "repo": approval.repo, "at": approval.at}),
            );
        }
        Ok(json!({"created": true, "factory": {"id": id, "project": project}}))
    }

    fn github_refusal(&mut self, failure: &Failure, stage: &str) -> Refusal {
        match failure.signal {
            Some(EnvSignal::GithubAuth) => {
                refuse("github_login_required", "Run gh auth login, then retry")
                    .with(json!({"stage": stage}))
            }
            Some(EnvSignal::GithubForbidden) => {
                let scope = failure
                    .missing_scope
                    .clone()
                    .unwrap_or_else(|| "repo".into());
                refuse(
                    "github_permission_missing",
                    format!("Run gh auth refresh -s {scope}, then retry"),
                )
                .with(json!({"stage": stage, "scope": scope}))
            }
            _ => refuse("github_unavailable", "Retry later").with(json!({"stage": stage})),
        }
    }

    // ---------------------------------------------------------------------- add

    fn validate_card(
        &self,
        factory: &str,
        own: Option<&str>,
        card: &CardInput,
        existing: Option<&Card>,
    ) -> Result<Card, Refusal> {
        let mut problems = Vec::new();
        let mut field = |name: &str, problem: &str| {
            problems.push(json!({"field": name, "problem": problem}));
        };
        let title = card
            .title
            .clone()
            .or_else(|| existing.map(|c| c.title.clone()))
            .unwrap_or_default();
        let goal = card
            .goal
            .clone()
            .or_else(|| existing.map(|c| c.goal.clone()))
            .unwrap_or_default();
        if title.trim().is_empty() {
            field("title", "missing");
        } else if title.chars().count() > TITLE_LIMIT || title.contains('\n') {
            field("title", "one line of at most 200 characters");
        }
        if goal.trim().is_empty() {
            field("goal", "missing");
        } else if goal.len() > TEXT_LIMIT {
            field("goal", "too long");
        }
        let criteria = if card.criteria.is_empty() {
            existing.map(|c| c.criteria.clone()).unwrap_or_default()
        } else {
            card.criteria.clone()
        };
        if criteria.iter().all(|c| c.trim().is_empty()) {
            field("criteria", "at least one completion criterion");
        }
        for (name, list) in [
            ("criteria", &criteria),
            ("out_of_scope", &card.out_of_scope),
            ("open_decisions", &card.open_decisions),
            ("depends_on", &card.depends_on),
            ("external", &card.external),
        ] {
            if list.len() > LIST_LIMIT {
                field(name, "too many items");
            }
            if list.iter().any(|item| item.len() > TEXT_LIMIT) {
                field(name, "an item is too long");
            }
        }
        if let Some(priority) = card.priority
            && !(-100..=100).contains(&priority)
        {
            field("priority", "between -100 and 100");
        }
        let tasks = self.tasks.get(factory);
        let mut depends = Vec::new();
        for reference in &card.depends_on {
            let found = tasks.and_then(|tasks| {
                tasks.values().find(|task| {
                    task.id == *reference
                        || task
                            .issue
                            .as_ref()
                            .is_some_and(|issue| issue.display() == *reference)
                })
            });
            match found {
                Some(task) if task.state == TaskState::Cancelled => {
                    field("depends_on", "a cancelled Task")
                }
                Some(task) => depends.push(task.id.clone()),
                None => field(
                    "depends_on",
                    &format!("{reference} does not exist in this Factory"),
                ),
            }
        }
        if let Some(own) = own {
            let mut edges = dag::edges(tasks.into_iter().flat_map(|t| t.values()));
            edges.insert(own.to_owned(), BTreeSet::new());
            for on in &depends {
                if let Some(path) = dag::cycle_with(&edges, own, on) {
                    field("depends_on", &format!("cycle: {}", path.join(" -> ")));
                }
                edges.entry(own.to_owned()).or_default().insert(on.clone());
            }
        } else {
            let mut seen = BTreeSet::new();
            for on in &depends {
                if !seen.insert(on) {
                    field("depends_on", "listed twice");
                }
            }
        }
        if !problems.is_empty() {
            return Err(
                refuse("card_invalid", "Fix the listed fields and add again")
                    .with(json!({"fields": problems})),
            );
        }
        Ok(Card {
            summary: existing
                .filter(|old| old.goal.trim() == goal.trim())
                .and_then(|old| old.summary.clone()),
            title: title.trim().to_owned(),
            goal: goal.trim().to_owned(),
            criteria: criteria
                .into_iter()
                .filter(|c| !c.trim().is_empty())
                .collect(),
            out_of_scope: if card.out_of_scope.is_empty() {
                existing.map(|c| c.out_of_scope.clone()).unwrap_or_default()
            } else {
                card.out_of_scope.clone()
            },
            // A re-add that leaves a list out keeps it, as for criteria.
            open_decisions: if card.open_decisions.is_empty() {
                existing
                    .map(|c| c.open_decisions.clone())
                    .unwrap_or_default()
            } else {
                card.open_decisions.clone()
            },
            depends_on: depends,
            external: if card.external.is_empty() {
                existing.map(|c| c.external.clone()).unwrap_or_default()
            } else {
                card.external.clone()
            },
        })
    }

    fn add(
        &mut self,
        role: &Role,
        project: Option<&str>,
        task_ref: Option<&str>,
        issue: Option<&str>,
        input: CardInput,
        producer_pane: Option<String>,
    ) -> Reply {
        // The Task named, or the one bound to the issue (B9, B14).
        let existing = match (task_ref, issue) {
            (Some(reference), _) => Some(self.resolve(role, reference)?),
            (None, Some(issue)) => self
                .all_tasks()
                .find(|task| task.issue.as_ref().is_some_and(|i| i.display() == issue))
                .map(|task| (task.factory.clone(), task.id.clone())),
            _ => None,
        };
        let factory_id = match &existing {
            Some((factory, _)) => factory.clone(),
            None => self.factory_id(project)?,
        };
        let factory = self.factories.get(&factory_id).cloned().ok_or_else(|| {
            refuse(
                "factory_not_found",
                "Create one with hide factory init <project>",
            )
        })?;
        if factory.closed {
            return Err(refuse(
                "factory_closed",
                "Reopen it with hide factory init <project> --confirm",
            ));
        }
        let now = self.now();
        if let Some((factory_id, id)) = existing {
            return self.readd(&factory, &factory_id, &id, input, producer_pane, now);
        }
        // A new Task, possibly bound to an existing issue.
        let issue_ref = match issue {
            Some(text) => Some(parse_issue(text, factory.source).ok_or_else(|| {
                refuse("issue_invalid", "Give an issue as #<number> or L-<number>")
            })?),
            None => None,
        };
        let mut input = input;
        if let Some(issue) = &issue_ref
            && (input.title.is_none() || input.goal.is_none())
        {
            match self.ports.source.read_issue(&factory, issue) {
                Ok(text) => {
                    input.title.get_or_insert(text.title);
                    if input.goal.is_none() && !text.body.trim().is_empty() {
                        input.goal = Some(judgment::cut(&text.body, TEXT_LIMIT));
                    }
                }
                Err(failure) => return Err(self.github_refusal(&failure, "read_issue")),
            }
        }
        if self.tasks.get(&factory_id).map_or(0, BTreeMap::len) >= TASK_LIMIT {
            return Err(refuse("capacity", "Cancel or finish Tasks first"));
        }
        let seq = factory.next_task;
        let id = format!("T-{seq}");
        let worker = worker_index(&factory, input.worker)?;
        let card = self.validate_card(&factory_id, Some(&id), &input, None)?;
        let mut task = Task::draft(&factory_id, &id, seq, card, now);
        task.issue = issue_ref;
        task.human = HumanFields {
            review_directly: input.review_directly,
            priority: input.priority.unwrap_or(0),
            merge_mode: input.merge_mode,
            runtime: input.runtime,
            worker,
        };
        task.producer_pane = producer_pane;
        if let Some(prd) = &input.prd {
            let attachment = self
                .ports
                .source
                .read_prd(prd)
                .and_then(|bytes| {
                    self.store
                        .attach(&factory_id, &id, Path::new(prd), &bytes, 1)
                        .map_err(|error| error.0)
                })
                .map_err(|error| {
                    refuse("attachment_failed", "Check the PRD path").with(json!({"error": error}))
                })?;
            task.attachments.push(attachment);
        }
        if let Some(factory) = self.factories.get_mut(&factory_id) {
            factory.next_task += 1;
        }
        self.save_factory(&factory_id);
        self.tasks
            .entry(factory_id.clone())
            .or_default()
            .insert(id.clone(), task);
        self.save(&factory_id, &id);
        self.record(&factory_id, Some(&id), "task.added", json!({"via": "add"}));
        for decision in input.open_decisions.iter().filter(|d| !d.trim().is_empty()) {
            self.add_question(
                &factory_id,
                &id,
                QuestionOrigin::Engine,
                QuestionKind::Intake,
                &format!("열린 결정: {decision}"),
                "결정을 적어 주세요",
                None,
                None,
                Vec::new(),
                None,
            );
        }
        self.request_review(&factory_id, &id, now);
        Ok(self.add_answer(&factory_id, &id))
    }

    fn readd(
        &mut self,
        factory: &Factory,
        factory_id: &str,
        id: &str,
        input: CardInput,
        producer_pane: Option<String>,
        now: UnixMs,
    ) -> Reply {
        let task = self
            .task(factory_id, id)
            .cloned()
            .ok_or_else(|| refuse("task_not_found", "Check hide factory status"))?;
        if task.state == TaskState::Cancelled {
            return Err(refuse(
                "task_cancelled",
                "Revive it first with hide factory revive",
            ));
        }
        let worker = worker_index(factory, input.worker)?;
        let mut card = self.validate_card(factory_id, Some(id), &input, Some(&task.card))?;
        // A producer adds dependencies but never removes one a review or a
        // person added: removal is a person's `dep remove` (D-02, D-08).
        let mut depends_on = task.card.depends_on.clone();
        for dependency in card.depends_on.drain(..) {
            if !depends_on.contains(&dependency) {
                depends_on.push(dependency);
            }
        }
        card.depends_on = depends_on;
        let attachment = match &input.prd {
            Some(prd) => {
                let version = task.attachments.last().map_or(1, |a| a.version + 1);
                let attachment = self
                    .ports
                    .source
                    .read_prd(prd)
                    .and_then(|bytes| {
                        self.store
                            .attach(factory_id, id, Path::new(prd), &bytes, version)
                            .map_err(|error| error.0)
                    })
                    .map_err(|error| {
                        refuse("attachment_failed", "Check the PRD path")
                            .with(json!({"error": error}))
                    })?;
                (task.attachments.last().map(|a| &a.sha256) != Some(&attachment.sha256))
                    .then_some(attachment)
            }
            None => None,
        };
        let unchanged = card == task.card && attachment.is_none();
        if task.state != TaskState::Drafting {
            // Running work: a changed card or PRD is a scope change a person
            // approves (B16); the same content changes nothing.
            if unchanged {
                return Ok(self.add_answer(factory_id, id));
            }
            if let Some(attachment) = &attachment {
                self.record(
                    factory_id,
                    Some(id),
                    "attachment.pending",
                    json!({"version": attachment.version}),
                );
            }
            // A newer re-add replaces a pending one rather than queueing two.
            self.with_task(factory_id, id, |task| {
                for question in task.questions.iter_mut().filter(|q| {
                    q.open() && matches!(&q.kind, QuestionKind::ScopeChange { change: Some(_) })
                }) {
                    question.answer = Some(Answer {
                        text: "replaced by a newer re-add".into(),
                        chose: None,
                        relayed_by: "re-add".into(),
                        at: now,
                    });
                }
            });
            self.add_question(
                factory_id,
                id,
                QuestionOrigin::Engine,
                QuestionKind::ScopeChange {
                    change: Some(Box::new(CardChange { card, attachment })),
                },
                "실행 중에 카드나 PRD가 바뀌었습니다. 새 범위를 승인할까요?",
                "approve",
                Some("지금 범위로 진행".into()),
                Some(now + factory.config.question_deadline_ms),
                vec!["approve".into(), "reject".into()],
                None,
            );
            return Ok(self.add_answer(factory_id, id));
        }
        if unchanged && matches!(task.review, ReviewState::Done { .. }) {
            return Ok(self.add_answer(factory_id, id));
        }
        self.with_task(factory_id, id, |task| {
            task.card = card;
            if let Some(attachment) = attachment {
                task.attachments.push(attachment);
            }
            if let Some(priority) = input.priority {
                task.human.priority = priority;
            }
            task.human.review_directly |= input.review_directly;
            if input.merge_mode.is_some() {
                task.human.merge_mode = input.merge_mode;
            }
            if input.runtime.is_some() {
                task.human.runtime = input.runtime;
            }
            if worker.is_some() {
                task.human.worker = worker;
            }
            if producer_pane.is_some() {
                task.producer_pane = producer_pane;
            }
            // The producer handled the review's questions in its
            // conversation; the new card is reviewed again (B9).
            for question in task.questions.iter_mut().filter(|q| q.open()) {
                if matches!(question.origin, QuestionOrigin::Review)
                    || matches!(
                        question.kind,
                        QuestionKind::Intake | QuestionKind::Split { .. }
                    )
                {
                    question.answer = Some(Answer {
                        text: "card updated".into(),
                        chose: None,
                        relayed_by: "re-add".into(),
                        at: now,
                    });
                }
            }
            task.review = ReviewState::Pending;
        });
        for decision in input.open_decisions.iter().filter(|d| !d.trim().is_empty()) {
            self.add_question(
                factory_id,
                id,
                QuestionOrigin::Engine,
                QuestionKind::Intake,
                &format!("열린 결정: {decision}"),
                "결정을 적어 주세요",
                None,
                None,
                Vec::new(),
                None,
            );
        }
        self.record(factory_id, Some(id), "task.updated", json!({"via": "add"}));
        self.request_review(factory_id, id, now);
        Ok(self.add_answer(factory_id, id))
    }

    /// What `add` answers now: ready, needs_answers, split, or pending while
    /// the review runs (the daemon waits up to 90 s for a final answer).
    pub fn add_answer(&self, factory: &str, id: &str) -> Value {
        let Some(task) = self.task(factory, id) else {
            return Refusal::new("task_not_found", "Check hide factory status").to_json();
        };
        let result = match (&task.review, task.state) {
            (_, state) if state != TaskState::Drafting => "ready",
            (
                ReviewState::Done {
                    result: ReviewResult::Split,
                },
                _,
            ) => "split",
            (ReviewState::Done { .. }, _)
                if task
                    .open_questions()
                    .any(|q| !matches!(q.kind, QuestionKind::Notice)) =>
            {
                "needs_answers"
            }
            (ReviewState::Done { .. }, _) => "ready",
            _ => "pending",
        };
        let questions: Vec<Value> = task
            .open_questions()
            .filter(|q| !matches!(q.kind, QuestionKind::Notice))
            .map(
                |q| json!({"id": q.id, "text": q.text, "suggestion": q.suggestion, "kind": q.kind}),
            )
            .collect();
        json!({
            "ok": true,
            "result": result,
            "task": {"id": task.id, "display_id": task.display_id(), "title": task.card.title, "state": task.state.as_str()},
            "questions": questions,
        })
    }

    pub fn review_settled(&self, factory: &str, id: &str) -> bool {
        self.task(factory, id).is_none_or(|task| {
            !matches!(
                task.review,
                ReviewState::Pending | ReviewState::Requested { .. }
            )
        })
    }

    fn request_review(&mut self, factory: &str, id: &str, now: UnixMs) {
        let Some(task) = self.task(factory, id).cloned() else {
            return;
        };
        // A paused Factory reviews nothing; the card waits as it is and is
        // reviewed when the Factory resumes (D-49).
        if self.factories.get(factory).is_some_and(|f| f.paused) {
            self.with_task(factory, id, |task| task.review = ReviewState::Pending);
            return;
        }
        // The review picks a worker candidate when there is a choice (D-41).
        let candidates = self
            .factories
            .get(factory)
            .map(|f| f.config.candidates())
            .unwrap_or_default();
        let workers = if candidates.len() > 1 {
            candidates
                .iter()
                .enumerate()
                .map(|(index, candidate)| judgment::CandidateNote {
                    index,
                    agent: candidate.agent.label().to_owned(),
                    model: candidate.model.clone(),
                    effort: candidate.effort.clone(),
                    description: candidate.description.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
        let attachment = task
            .attachments
            .last()
            .and_then(|a| std::fs::read_to_string(&a.path).ok());
        let other_tasks = self
            .tasks_of(factory)
            .filter(|other| other.id != id && other.state != TaskState::Cancelled)
            .take(100)
            .map(|other| OtherTask {
                id: other.id.clone(),
                title: other.card.title.clone(),
                goal: judgment::cut(&other.card.goal, 400),
                state: other.state.as_str().to_owned(),
            })
            .collect();
        let project = self
            .factories
            .get(factory)
            .map(|f| f.project.clone())
            .unwrap_or_default();
        let RepoContext {
            files: repo_files,
            guide,
        } = self.ports.source.repo_context(&project);
        let judgment_id = format!(
            "{factory}:{id}:intake:{}",
            sha256_hex(
                serde_json::to_string(&task.card)
                    .unwrap_or_default()
                    .as_bytes()
            )
            .get(..12)
            .unwrap_or("")
        );
        let mut checks = Vec::new();
        if let Some(f) = self.factories.get(factory) {
            for check in f
                .config
                .checks
                .iter()
                .filter(|c| c.at == CheckPoint::Intake)
            {
                checks.push(check.instruction.clone());
            }
        }
        let judgment = Judgment {
            id: judgment_id.clone(),
            factory: factory.to_owned(),
            task: Some(id.to_owned()),
            priority: Priority::Intake,
            input: JudgmentInput::IntakeReview {
                card: task.card.clone(),
                attachment,
                other_tasks,
                repo_files,
                guide,
                autonomy_scope: self.autonomy_scope(factory, &task),
                workers,
            },
            ai: None,
        };
        self.with_task(factory, id, |task| {
            task.review = ReviewState::Requested { at: now }
        });
        match self.submit_judgment(judgment) {
            Ok(()) => {
                self.judgments.insert(
                    judgment_id,
                    (factory.to_owned(), Some(id.to_owned()), Purpose::Intake),
                );
            }
            Err(failure) => self.review_failed(factory, id, &failure.detail),
        }
        for (index, instruction) in checks.into_iter().enumerate() {
            let check_id = format!("{factory}:{id}:intake-check:{index}:{now}");
            let judgment = Judgment {
                id: check_id.clone(),
                factory: factory.to_owned(),
                task: Some(id.to_owned()),
                priority: Priority::Factory,
                input: JudgmentInput::Check {
                    instruction,
                    card: task.card.clone(),
                    diff: None,
                },
                ai: None,
            };
            if self.submit_judgment(judgment).is_ok() {
                self.judgments.insert(
                    check_id,
                    (factory.to_owned(), Some(id.to_owned()), Purpose::Check),
                );
            }
        }
    }

    /// The review could not run: never skipped and never ready (B19).
    fn review_failed(&mut self, factory: &str, id: &str, reason: &str) {
        self.with_task(factory, id, |task| {
            task.review = ReviewState::Failed {
                reason: reason.to_owned(),
            }
        });
        self.record(
            factory,
            Some(id),
            "review.failed",
            json!({"reason": reason}),
        );
        let already = self.task(factory, id).is_some_and(|task| {
            task.open_questions()
                .any(|q| q.text.starts_with("검토를 하지 못했습니다"))
        });
        if !already {
            // Hide AI off, or no agent chosen to run it, refuses every
            // judgment as `disabled`; fixing a provider would not help.
            let text = if reason == "disabled" {
                "검토를 하지 못했습니다. Settings › Hide AI에서 Hide AI를 켜고 Runs on을 고른 뒤 다시 검토하세요."
            } else {
                "검토를 하지 못했습니다. Settings › Hide AI에서 provider를 고친 뒤 다시 검토하세요."
            };
            self.add_question(
                factory,
                id,
                QuestionOrigin::Engine,
                QuestionKind::Action,
                text,
                "retry-review",
                None,
                None,
                vec!["retry-review".into(), "cancel".into()],
                None,
            );
        }
        self.pending_to_producer(factory, id);
    }

    fn pending_to_producer(&mut self, factory: &str, id: &str) {
        let Some(task) = self.task(factory, id) else {
            return;
        };
        let Some(pane) = task.producer_pane.clone() else {
            return;
        };
        let answer = self.add_answer(factory, id);
        let body = format!(
            "Factory review of {}: {}",
            answer["task"]["display_id"].as_str().unwrap_or(id),
            answer["result"].as_str().unwrap_or("pending")
        );
        if !self.ports.notifier.producer(factory, &pane, &body) {
            self.with_task(factory, id, |task| task.producer_pane = None);
        }
    }

    /// The description of the enabled scope a Task claims.
    fn autonomy_scope(&self, factory: &str, task: &Task) -> Option<String> {
        let scope = task.autonomy.as_ref()?;
        self.factories
            .get(factory)?
            .config
            .autonomy
            .iter()
            .find(|s| s.id == *scope && s.enabled)
            .map(|s| s.description.clone())
    }

    fn apply_intake(&mut self, factory: &str, id: &str, value: &Value) {
        let verdict = match judgment::parse_intake(value) {
            Ok(verdict) => verdict,
            Err(reason) => return self.review_failed(factory, id, &reason),
        };
        let mut result = verdict.result();
        let choice = self
            .factories
            .get(factory)
            .map_or(0, |f| f.config.candidates().len());
        let pick = verdict
            .worker
            .clone()
            .filter(|pick| choice > 1 && pick.index < choice);
        self.with_task(factory, id, |task| {
            if pick.is_some() {
                task.ai_pick = pick;
            }
            if task.card.summary.is_none() {
                task.card.summary = Some(
                    verdict
                        .summary
                        .as_deref()
                        .map(short_summary)
                        .filter(|summary| !summary.is_empty())
                        .unwrap_or_else(|| goal_summary(&task.card.goal, &task.card.title)),
                );
            }
        });
        let now = self.now();
        // A worker's claim is not the fit: only a review that says the card
        // fits its enabled scope lets it start without a person (B30).
        if let Some(task) = self.task(factory, id).cloned()
            && task.autonomy.is_some()
            && verdict.fits_scope != Some(true)
        {
            let scope = self
                .autonomy_scope(factory, &task)
                .unwrap_or_else(|| task.autonomy.clone().unwrap_or_default());
            self.with_task(factory, id, |task| task.autonomy = None);
            self.record(factory, Some(id), "review.outside_scope", json!({}));
            self.add_question(
                factory,
                id,
                QuestionOrigin::Review,
                QuestionKind::Intake,
                &format!(
                    "worker가 자율 처리 범위 '{}'로 제안한 Task인데 검토가 범위에 맞다고 보지 않았습니다. 사람이 승인하면 진행합니다.",
                    judgment::cut(&scope, 200)
                ),
                "진행",
                None,
                None,
                Vec::new(),
                None,
            );
            result = ReviewResult::NeedsAnswers;
        }
        // The review only adds (D-02, B8): questions, dependencies, flags.
        for question in &verdict.questions {
            self.add_question(
                factory,
                id,
                QuestionOrigin::Review,
                QuestionKind::Intake,
                &question.text,
                &question.suggestion,
                question.default_action.clone(),
                None,
                Vec::new(),
                None,
            );
        }
        let candidates: Vec<String> = verdict
            .dependencies
            .iter()
            .filter_map(|reference| {
                self.tasks_of(factory)
                    .find(|task| {
                        task.id == *reference
                            || task
                                .issue
                                .as_ref()
                                .is_some_and(|i| i.display() == *reference)
                    })
                    .filter(|task| task.id != id && task.state != TaskState::Cancelled)
                    .map(|task| task.id.clone())
            })
            .collect();
        for on in candidates {
            let edges = dag::edges(self.tasks_of(factory));
            if dag::cycle_with(&edges, id, &on).is_some() {
                self.record(
                    factory,
                    Some(id),
                    "review.dependency_refused",
                    json!({"on": on, "reason": "cycle"}),
                );
                continue;
            }
            self.with_task(factory, id, |task| {
                if !task.card.depends_on.contains(&on) {
                    task.card.depends_on.push(on.clone());
                }
            });
        }
        if !verdict.split.is_empty() {
            self.add_question(
                factory,
                id,
                QuestionOrigin::Review,
                QuestionKind::Split {
                    pieces: verdict.split.clone(),
                },
                &format!("이 Task를 {}개로 쪼갤까요?", verdict.split.len()),
                "split",
                None,
                None,
                vec!["split".into(), "proceed".into()],
                None,
            );
        }
        self.with_task(factory, id, |task| {
            task.flags.extend(verdict.flags.iter().cloned());
            task.review = ReviewState::Done { result };
        });
        self.record(
            factory,
            Some(id),
            "review.done",
            json!({"result": format!("{result:?}"), "at": now}),
        );
        self.maybe_ready(factory, id);
        self.pending_to_producer(factory, id);
    }

    /// Drafting -> Waiting when the review is done and nothing is open (B12).
    fn maybe_ready(&mut self, factory_id: &str, id: &str) {
        let Some(task) = self.task(factory_id, id).cloned() else {
            return;
        };
        if task.state != TaskState::Drafting
            || !matches!(task.review, ReviewState::Done { .. })
            || task
                .open_questions()
                .any(|q| !matches!(q.kind, QuestionKind::Notice))
        {
            return;
        }
        let Some(factory) = self.factories.get(factory_id).cloned() else {
            return;
        };
        // Ready creates the issue (B13); a Task added from an issue gets the label.
        let key = "issue";
        if task.issue.is_none() {
            if !task.writes.contains(key) {
                self.with_task(factory_id, id, |task| {
                    task.writes.insert(key.to_owned());
                });
            }
            let body = issue_body(&task, &factory);
            match self.ports.source.create_issue(&factory, &task, &body) {
                Ok(issue) => {
                    self.record(
                        factory_id,
                        Some(id),
                        "github.issue_created",
                        json!({"issue": issue.display()}),
                    );
                    if let IssueRef::Local { number } = issue
                        && let Some(f) = self.factories.get_mut(factory_id)
                    {
                        f.next_local_issue = f.next_local_issue.max(number + 1);
                    }
                    self.save_factory(factory_id);
                    // The body it wrote is the one a later edit is read
                    // against, so the first edit a person makes counts.
                    let written = crate::store::body_hash(&body);
                    self.with_task(factory_id, id, |task| {
                        task.issue = Some(issue);
                        task.source_body_hash = Some(written);
                    });
                }
                Err(failure) => {
                    self.external_failure(factory_id, Some(id), &failure);
                    return;
                }
            }
        } else if factory.source == SourceKind::Github && !task.writes.contains("label") {
            let issue = task.issue.clone().unwrap_or(IssueRef::Github { number: 0 });
            match self.ports.source.label_issue(&factory, &issue) {
                Ok(()) => {
                    self.with_task(factory_id, id, |task| {
                        task.writes.insert("label".into());
                    });
                }
                Err(failure) => {
                    self.external_failure(factory_id, Some(id), &failure);
                    return;
                }
            }
        }
        self.set_state(factory_id, id, TaskState::Waiting);
    }

    // ------------------------------------------------------------------- answer

    #[allow(clippy::too_many_arguments)]
    fn answer(
        &mut self,
        role: &Role,
        factory: &str,
        id: &str,
        question: Option<&str>,
        choice: Option<String>,
        text: Option<String>,
        change: bool,
    ) -> Reply {
        let task = self
            .task(factory, id)
            .cloned()
            .ok_or_else(|| refuse("task_not_found", "Check hide factory status"))?;
        // "다른 답" on a request the Observer answered (D-19); a plain answer
        // to an answered question is refused, so the loser of a race with the
        // Observer changes nothing.
        if let Some(qid) = question
            && let Some(answered) = task.questions.iter().find(|q| q.id == qid && !q.open())
        {
            if change {
                return self.override_answer(role, factory, id, answered.clone(), choice, text);
            }
            return Err(refuse(
                "already_answered",
                "This question was answered already; check hide factory show",
            )
            .with(json!({"by": answered.answer.as_ref().map(|a| a.relayed_by.clone())})));
        }
        let open: Vec<&Question> = task.open_questions().collect();
        let target = match question {
            Some(qid) => open.iter().find(|q| q.id == qid).copied(),
            None => open.first().copied(),
        };
        let Some(target) = target.cloned() else {
            return Err(refuse(
                "no_open_question",
                "This Task has no open question with that id",
            )
            .with(json!({"state": task.state.label(), "allowed": Self::allowed_actions(&task)})));
        };
        let chosen = match choice.as_deref() {
            Some("suggestion") => Some(target.suggestion.clone()),
            Some("default") => target.default_action.clone(),
            Some(other) => Some(other.to_owned()),
            None => None,
        };
        let text = text.or_else(|| chosen.clone()).unwrap_or_default();
        if text.trim().is_empty() {
            return Err(refuse(
                "answer_required",
                "Pass --choose suggestion|default|<choice> or --text",
            ));
        }
        if !target.choices.is_empty()
            && let Some(choice) = &chosen
            && !target.choices.contains(choice)
            && choice != &target.suggestion
            && Some(choice) != target.default_action.as_ref()
        {
            return Err(refuse("choice_invalid", "Choose one of the listed choices")
                .with(json!({"allowed": target.choices})));
        }
        let relayed_by = role.relayed_by();
        // The Observer's fix for a wrong card, chosen by a person (D-33).
        if chosen.as_deref() == Some(PROPOSAL_CHOICE)
            && let Some(proposal) = target.routing.as_ref().and_then(|r| r.proposal.clone())
        {
            self.apply_proposal(factory, id, &target, proposal, &relayed_by, None);
            return Ok(self.task_answer(factory, id, "answered"));
        }
        self.settle_answer(factory, id, &target, &text, chosen, &relayed_by, None);
        Ok(self.task_answer(factory, id, "answered"))
    }

    /// Records an answer and sends it where it goes: the one path a person's
    /// answer and the Observer's share (B11). `observer` carries the kind and
    /// reason of an answer the Observer gave.
    #[allow(clippy::too_many_arguments)]
    fn settle_answer(
        &mut self,
        factory: &str,
        id: &str,
        target: &Question,
        text: &str,
        chosen: Option<String>,
        relayed_by: &str,
        observer: Option<(DecisionKind, String)>,
    ) {
        let Some(task) = self.task(factory, id).cloned() else {
            return;
        };
        let target = target.clone();
        let text = text.to_owned();
        let relayed_by = relayed_by.to_owned();
        let now = self.now();
        let answer = Answer {
            text: judgment::cut(&text, TEXT_LIMIT),
            chose: chosen.clone(),
            relayed_by: relayed_by.clone(),
            at: now,
        };
        self.with_task(factory, id, |task| {
            if let Some(question) = task.questions.iter_mut().find(|q| q.id == target.id) {
                question.answer = Some(answer.clone());
                if let Some(routing) = &mut question.routing {
                    match &observer {
                        Some((kind, reason)) => {
                            routing.to = RouteTo::Observer;
                            routing.kind = Some(*kind);
                            routing.reason = Some(reason.clone());
                        }
                        None if routing.to == RouteTo::Pending => routing.to = RouteTo::Person,
                        None => {}
                    }
                }
            }
            task.decisions.push(DecisionRecord {
                text: format!("{} -> {}", judgment::cut(&target.text, 200), answer.text),
                by: relayed_by.clone(),
                at: now,
                kind: observer.as_ref().map(|(kind, _)| *kind),
                reason: observer.as_ref().map(|(_, reason)| reason.clone()),
            });
        });
        self.record(
            factory,
            Some(id),
            "question.answered",
            json!({"question": target.id, "by": relayed_by}),
        );
        let decision = chosen.as_deref().unwrap_or(text.as_str()).to_owned();
        match &target.kind {
            QuestionKind::ConfirmCard if decision == "cancel" => self.cancel(factory, id),
            QuestionKind::Intake | QuestionKind::ConfirmCard => self.maybe_ready(factory, id),
            QuestionKind::Split { pieces } => {
                if decision == "split" || decision == target.suggestion {
                    self.split(factory, id, pieces.clone());
                } else {
                    self.with_task(factory, id, |task| {
                        task.flags.retain(|flag| flag != "split")
                    });
                    self.maybe_ready(factory, id);
                }
            }
            QuestionKind::Default => {
                if Some(&decision) != target.default_action.as_ref() {
                    let who = if relayed_by == OBSERVER {
                        "Factory AI가"
                    } else {
                        "사람이"
                    };
                    let body = format!(
                        "Factory: {who} 기본 행동과 다르게 답했습니다: {text}\n이 답을 반영한 뒤 다시 hide factory done 하세요."
                    );
                    let finished = self.task(factory, id).is_some_and(|t| {
                        matches!(t.state, TaskState::Verifying | TaskState::MergeWaiting)
                    });
                    if finished {
                        // The worker reported done; the answer sends it back.
                        self.cancel_verification(factory, id);
                        self.set_state(factory, id, TaskState::Running);
                        self.wake(factory, id, &body);
                    } else {
                        self.reply(factory, id, target.letter.as_deref(), &body);
                    }
                }
                self.advance_merge(factory, id);
            }
            QuestionKind::ScopeChange { change } => {
                let approved = decision == "approve";
                let mut prd = None;
                self.with_task(factory, id, |task| {
                    if approved {
                        task.scope_approved = true;
                        if let Some(change) = change {
                            task.card = change.card.clone();
                            if let Some(attachment) = &change.attachment {
                                prd = Some(format!(
                                    "\nPRD v{}: {} (읽기 전용)",
                                    attachment.version, attachment.path
                                ));
                                task.attachments.push(attachment.clone());
                            }
                        }
                    }
                });
                let card = match (approved, change) {
                    (true, Some(_)) => self
                        .task(factory, id)
                        .map(|t| format!("\n새 카드:\n{}", card_text(&t.card)))
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                let prd = prd.unwrap_or_default();
                self.reply(factory, id, target.letter.as_deref(), &if approved {
                    format!("Factory: 범위 변경이 승인되었습니다. {text}{card}{prd}")
                } else {
                    format!("Factory: 범위 변경은 승인되지 않았습니다. 지금 범위 안에서 진행하세요. {text}")
                });
                self.advance_merge(factory, id);
            }
            QuestionKind::Blocking => {
                self.reply(
                    factory,
                    id,
                    target.letter.as_deref(),
                    &format!("Factory: 답이 왔습니다: {text}\n이어서 진행하세요."),
                );
                if task.state == TaskState::Blocked
                    && task
                        .open_questions()
                        .filter(|q| matches!(q.kind, QuestionKind::Blocking))
                        .count()
                        <= 1
                {
                    // Waits for a slot like any start, then wakes the same session (B27).
                    self.set_state(factory, id, TaskState::Waiting);
                }
            }
            QuestionKind::NewTaskCap => {
                match decision.as_str() {
                    "continue" => {
                        self.with_task(factory, id, |task| task.new_task_cap_extended = true);
                        self.set_state(factory, id, TaskState::Waiting);
                    }
                    _ => {
                        self.notice(factory, id, "새 Task를 더 만들지 않고 멈춘 상태로 둡니다. 다시 시도하거나 취소하세요.");
                    }
                }
            }
            QuestionKind::ProposedTask { draft, discovery } => {
                if decision == "approve" {
                    let new_id = self.create_child(factory, id, (**draft).clone(), None);
                    self.record(
                        factory,
                        Some(id),
                        "proposal.approved",
                        json!({"task": new_id}),
                    );
                    if let Some(discovery) = discovery {
                        self.wait_on_prerequisite(factory, id, discovery, &new_id);
                    }
                }
            }
            QuestionKind::Action => match decision.as_str() {
                "retry-review" => {
                    self.with_task(factory, id, |task| task.review = ReviewState::Pending);
                    self.request_review(factory, id, now);
                }
                "retry" if task.state == TaskState::Stopped => {
                    self.with_task(factory, id, |task| {
                        task.auto_restarts = 0;
                        task.recovery = None;
                        task.diagnosis = None;
                    });
                    self.set_state(factory, id, TaskState::Waiting);
                }
                "cancel" => self.cancel(factory, id),
                "resume-auto" => {
                    if let Some(f) = self.factories.get_mut(factory) {
                        f.main.broken = false;
                        f.main.needs_person = false;
                        f.main.reason = None;
                    }
                    self.save_factory(factory);
                }
                "retry-revert" => {
                    if let Some(f) = self.factories.get_mut(factory) {
                        f.main.needs_person = false;
                    }
                    self.set_recovery(factory, None);
                    self.save_factory(factory);
                }
                other if other.starts_with("revert ") => {
                    let target_task = other.trim_start_matches("revert ").to_owned();
                    if let Some(f) = self.factories.get_mut(factory) {
                        f.main.needs_person = false;
                    }
                    self.save_factory(factory);
                    self.start_revert(factory, &target_task);
                }
                _ => {}
            },
            QuestionKind::Proposal { command, .. } => {
                // A person runs a proposed command themselves; the Factory
                // runs only its typed recovery actions, once approved (B61,
                // D-54).
                self.record(
                    factory,
                    Some(id),
                    "proposal.answered",
                    json!({"decision": decision}),
                );
                if decision == "approve"
                    && let Some(action) = RecoveryAction::ALL
                        .into_iter()
                        .find(|action| action.as_str() == command)
                {
                    self.run_recovery(factory, action);
                }
            }
            QuestionKind::Notice => {
                if task.state == TaskState::Done {
                    self.with_task(factory, id, |task| task.seen = true);
                }
            }
        }
    }

    fn split(&mut self, factory: &str, id: &str, pieces: Vec<SplitPiece>) {
        let Some(first) = pieces.first().cloned() else {
            return;
        };
        // The original becomes the first piece and keeps its issue (D-55).
        self.with_task(factory, id, |task| {
            task.card.title = first.title.clone();
            task.card.summary = None;
            task.card.goal = first.goal.clone();
            task.card.criteria = first.criteria.clone();
            task.review = ReviewState::Pending;
        });
        let mut ids = vec![id.to_owned()];
        for piece in pieces.iter().skip(1) {
            let mut depends: Vec<String> = piece
                .after
                .iter()
                .filter_map(|index| ids.get(*index).cloned())
                .collect();
            if depends.is_empty() {
                depends.push(id.to_owned());
            }
            let card = Card {
                title: piece.title.clone(),
                summary: None,
                goal: piece.goal.clone(),
                criteria: piece.criteria.clone(),
                out_of_scope: Vec::new(),
                open_decisions: Vec::new(),
                depends_on: depends,
                external: Vec::new(),
            };
            let new_id = self.new_task(factory, card, None, None);
            ids.push(new_id.clone());
            let now = self.now();
            self.request_review(factory, &new_id, now);
        }
        let now = self.now();
        self.request_review(factory, id, now);
    }

    fn new_task(
        &mut self,
        factory: &str,
        card: Card,
        proposed_by: Option<String>,
        autonomy: Option<String>,
    ) -> String {
        let now = self.now();
        let seq = self.factories.get(factory).map_or(1, |f| f.next_task);
        let id = format!("T-{seq}");
        if let Some(f) = self.factories.get_mut(factory) {
            f.next_task += 1;
        }
        self.save_factory(factory);
        let mut task = Task::draft(factory, &id, seq, card, now);
        task.proposed_by = proposed_by;
        task.autonomy = autonomy;
        self.tasks
            .entry(factory.to_owned())
            .or_default()
            .insert(id.clone(), task);
        self.save(factory, &id);
        self.record(factory, Some(&id), "task.added", json!({"via": "engine"}));
        id
    }

    /// The proposer waits on the prerequisite it found and gives its slot
    /// back (D-16 ④); `unblock` starts it again once the prerequisite landed.
    /// A Task already past its work keeps going: the new Task is only linked.
    fn wait_on_prerequisite(&mut self, factory: &str, id: &str, discovery: &str, new_id: &str) {
        let mut waits = false;
        self.with_task(factory, id, |task| {
            if let Some(found) = task.discoveries.iter_mut().find(|d| d.id == discovery) {
                found.task = Some(new_id.to_owned());
            }
            waits = matches!(
                task.state,
                TaskState::Running | TaskState::Waiting | TaskState::Blocked | TaskState::Paused
            );
            if waits && !task.card.depends_on.iter().any(|d| d == new_id) {
                task.card.depends_on.push(new_id.to_owned());
            }
        });
        if waits
            && self
                .task(factory, id)
                .is_some_and(|t| t.state == TaskState::Running)
        {
            self.put_to_sleep(factory, id);
            self.set_state(factory, id, TaskState::Blocked);
        }
    }

    fn create_child(
        &mut self,
        factory: &str,
        parent: &str,
        card: Card,
        autonomy: Option<String>,
    ) -> String {
        let id = self.new_task(factory, card, Some(parent.to_owned()), autonomy);
        let now = self.now();
        self.request_review(factory, &id, now);
        id
    }

    // ---------------------------------------------------------- worker reports

    #[allow(clippy::too_many_arguments)]
    fn ask(
        &mut self,
        factory: &str,
        id: &str,
        text: &str,
        suggestion: &str,
        default_action: Option<String>,
        deadline_hours: Option<u64>,
        letter: Option<String>,
        choices: Vec<String>,
    ) -> Reply {
        let task = self
            .task(factory, id)
            .cloned()
            .ok_or_else(|| refuse("task_not_found", "Check hide factory status"))?;
        if text.trim().is_empty() {
            return Err(refuse("question_required", "Give --question"));
        }
        // A decision request carries at most five short choices (B1).
        let choices = judgment::valid_choices(choices).map_err(|(reason, limit)| {
            refuse(
                &reason,
                format!(
                    "Give at most {} choices of at most {} characters each",
                    judgment::CHOICE_LIMIT,
                    judgment::CHOICE_CHARS
                ),
            )
            .with(json!({"limit": limit}))
        })?;
        // A question must carry a suggestion and a deadline (B28).
        if suggestion.trim().is_empty() {
            return Err(refuse(
                "suggestion_required",
                "Give --suggestion with what you propose",
            ));
        }
        let Some(hours) = deadline_hours.filter(|h| *h > 0 && *h <= 24 * 30) else {
            return Err(refuse(
                "deadline_required",
                "Give --deadline-hours <n> (the Factory default is 24)",
            ));
        };
        if default_action.as_ref().is_some_and(|d| d.trim().is_empty()) {
            return Err(refuse(
                "default_required",
                "Give --default with the action you take meanwhile",
            ));
        }
        if !matches!(task.state, TaskState::Running | TaskState::Relanding) {
            return Err(refuse(
                "action_not_allowed_in_state",
                "A worker asks while its Task runs",
            )
            .with(json!({"state": task.state.label()})));
        }
        let now = self.now();
        let deadline = now + hours * HOUR_MS;
        let blocking = default_action.is_none();
        let question = self.add_question(
            factory,
            id,
            QuestionOrigin::Worker,
            if blocking {
                QuestionKind::Blocking
            } else {
                QuestionKind::Default
            },
            text,
            suggestion,
            default_action.clone(),
            Some(deadline),
            choices,
            letter,
        );
        self.with_task(factory, id, |task| task.last_report_at = Some(now));
        if blocking {
            // A question it cannot work past releases the slot and sleeps (B27).
            self.put_to_sleep(factory, id);
            self.set_state(factory, id, TaskState::Blocked);
        }
        // The Observer sorts it; the mode decides who answers (D-14).
        self.route_request(factory, id, &question);
        if blocking {
            return Ok(
                json!({"message": "blocked: the Task waits for the answer; your session is woken with it", "question": question}),
            );
        }
        Ok(json!({
            "message": format!("continue with the default action: {}", default_action.unwrap_or_default()),
            "question": question,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn propose(
        &mut self,
        factory: &str,
        id: &str,
        class: DiscoveryClass,
        text: &str,
        card: Option<CardInput>,
        autonomy: Option<String>,
        reclassify: Option<String>,
        letter: Option<String>,
    ) -> Reply {
        let task = self
            .task(factory, id)
            .cloned()
            .ok_or_else(|| refuse("task_not_found", "Check hide factory status"))?;
        if text.trim().is_empty() {
            return Err(refuse("text_required", "Give --text"));
        }
        let now = self.now();
        if let Some(previous) = reclassify {
            let Some(discovery) = task.discoveries.iter().find(|d| d.id == previous) else {
                return Err(refuse(
                    "discovery_not_found",
                    "Name a discovery of this Task",
                ));
            };
            // Only toward a person (B30).
            if class.toward_person() < discovery.class.toward_person() {
                return Err(refuse(
                    "reclassify_away_from_person",
                    "A discovery may only move toward a person",
                )
                .with(
                    json!({"from": format!("{:?}", discovery.class), "to": format!("{class:?}")}),
                ));
            }
        }
        let discovery_id = format!("D{}", task.discoveries.len() + 1);
        self.with_task(factory, id, |task| {
            task.discoveries.push(Discovery {
                id: discovery_id.clone(),
                class,
                text: judgment::cut(text, TEXT_LIMIT),
                at: now,
                task: None,
            });
            task.last_report_at = Some(now);
        });
        let deadline = self
            .factories
            .get(factory)
            .map_or(24 * HOUR_MS, |f| f.config.question_deadline_ms);
        match class {
            DiscoveryClass::InScope => {
                Ok(json!({"message": "recorded", "discovery": discovery_id}))
            }
            DiscoveryClass::Decision => {
                self.with_task(factory, id, |task| {
                    task.decisions.push(DecisionRecord {
                        text: judgment::cut(text, TEXT_LIMIT),
                        by: format!("worker:{id}"),
                        at: now,
                        kind: None,
                        reason: None,
                    })
                });
                Ok(json!({"message": "decision recorded", "discovery": discovery_id}))
            }
            DiscoveryClass::ScopeChange => {
                let question = self.add_question(
                    factory,
                    id,
                    QuestionOrigin::Worker,
                    QuestionKind::ScopeChange { change: None },
                    &format!("범위 변경: {text}"),
                    "approve",
                    Some("범위를 넓히지 않고 진행".into()),
                    Some(now + deadline),
                    vec!["approve".into(), "reject".into()],
                    letter,
                );
                Ok(
                    json!({"message": "continue within the current scope; a person decides the wider scope", "question": question}),
                )
            }
            DiscoveryClass::Unrelated => {
                self.notice(factory, id, &format!("무관한 발견: {text}"));
                Ok(json!({"message": "sent to the person's inbox", "discovery": discovery_id}))
            }
            DiscoveryClass::Prerequisite => {
                // Depth 1: a Task a worker proposed cannot propose (B30).
                if task.proposed_by.is_some() || task.autonomy.is_some() {
                    return Err(refuse(
                        "proposal_depth_exceeded",
                        "Report it as a scope change or an unrelated finding instead",
                    ));
                }
                let input = card.unwrap_or_default();
                let draft = self.validate_card(factory, None, &input, None)?;
                let limit = self
                    .factories
                    .get(factory)
                    .map_or(3, |f| f.config.new_task_limit);
                if task.new_tasks >= limit && !task.new_task_cap_extended {
                    return Err(refuse(
                        "new_task_limit",
                        "A person decides whether this Task may create more",
                    ));
                }
                let enabled = autonomy.as_ref().and_then(|scope| {
                    self.factories.get(factory).and_then(|f| {
                        f.config
                            .autonomy
                            .iter()
                            .find(|s| s.id == *scope && s.enabled)
                            .map(|s| s.id.clone())
                    })
                });
                let (created, message) = match enabled {
                    Some(scope) => {
                        let new_id = self.create_child(factory, id, draft, Some(scope));
                        (
                            Some(new_id),
                            "a new Task starts on its own within the enabled autonomy scope",
                        )
                    }
                    None => {
                        self.add_question(
                            factory,
                            id,
                            QuestionOrigin::Worker,
                            QuestionKind::ProposedTask {
                                draft: Box::new(draft.clone()),
                                discovery: Some(discovery_id.clone()),
                            },
                            &format!("새 Task 제안: {}", draft.title),
                            "approve",
                            None,
                            None,
                            vec!["approve".into(), "reject".into()],
                            letter,
                        );
                        (None, "proposed; a person approves before it is drafted")
                    }
                };
                self.with_task(factory, id, |task| task.new_tasks += 1);
                if let Some(new_id) = &created {
                    self.wait_on_prerequisite(factory, id, &discovery_id, new_id);
                }
                let reached = self
                    .task(factory, id)
                    .is_some_and(|t| t.new_tasks >= limit && !t.new_task_cap_extended);
                if reached {
                    // Reaching the cap is a watch event (B69).
                    self.watch_due(factory, true);
                    self.put_to_sleep(factory, id);
                    self.with_task(factory, id, |task| task.stop = Some(StopReason::NewTaskCap));
                    self.set_state(factory, id, TaskState::Stopped);
                    self.with_task(factory, id, |task| task.stop = Some(StopReason::NewTaskCap));
                    self.add_question(
                        factory,
                        id,
                        QuestionOrigin::Engine,
                        QuestionKind::NewTaskCap,
                        &format!(
                            "새 Task가 {limit}개 생겼습니다. 쪼갤까요 / 계속할까요 / 멈출까요"
                        ),
                        "split",
                        None,
                        None,
                        vec!["split".into(), "continue".into(), "stop".into()],
                        None,
                    );
                }
                Ok(json!({"message": message, "task": created, "discovery": discovery_id}))
            }
        }
    }

    fn done(
        &mut self,
        factory_id: &str,
        id: &str,
        summary_text: Option<String>,
        breaking: bool,
        letter: Option<String>,
    ) -> Reply {
        let task = self
            .task(factory_id, id)
            .cloned()
            .ok_or_else(|| refuse("task_not_found", "Check hide factory status"))?;
        if !matches!(task.state, TaskState::Running | TaskState::Relanding) {
            return Err(refuse(
                "action_not_allowed_in_state",
                "done is reported while the Task runs",
            )
            .with(json!({"state": task.state.label()})));
        }
        let now = self.now();
        let factory = self
            .factories
            .get(factory_id)
            .cloned()
            .ok_or_else(|| refuse("factory_not_found", "?"))?;
        self.with_task(factory_id, id, |task| {
            task.last_report_at = Some(now);
            task.breaking |= breaking;
            let full = task.questions.len() + task.decisions.len() + task.discoveries.len()
                >= REPORT_LIMIT;
            if let Some(text) = summary_text.as_ref().filter(|_| !full) {
                task.decisions.push(DecisionRecord {
                    text: format!("done: {}", judgment::cut(text, TEXT_LIMIT)),
                    by: format!("worker:{id}"),
                    at: now,
                    kind: None,
                    reason: None,
                });
            }
            task.gates.clear();
        });
        let _ = letter;
        self.put_to_sleep(factory_id, id);
        self.set_state(factory_id, id, TaskState::Verifying);
        if factory.source == SourceKind::Github {
            // B35, B36: the reply comes now; the next tick pushes this report's
            // commits and opens the pull request, then reads CI.
            self.with_task(factory_id, id, |task| {
                task.writes.insert(PUBLISH_PENDING.into());
            });
            self.publish_retry
                .remove(&(factory_id.to_owned(), id.to_owned()));
            return Ok(
                json!({"message": "검증 중: 결과는 편지로 갑니다. 지금은 차례를 끝내세요.", "state": "verifying"}),
            );
        }
        self.begin_verification(factory_id, id)
    }

    /// Pushes a GitHub Task's branch and finds or opens its pull request,
    /// then starts the checks and verification on the pushed commit (B36).
    fn publish(&mut self, factory_id: &str, id: &str) {
        let key = (factory_id.to_owned(), id.to_owned());
        let now = self.now();
        if self.publish_retry.get(&key).is_some_and(|at| *at > now) {
            return;
        }
        let (Some(factory), Some(task)) = (
            self.factories.get(factory_id).cloned(),
            self.task(factory_id, id).cloned(),
        ) else {
            return;
        };
        let body = pr_body(&task, &factory);
        match self.ports.merge.open_pr(&factory, &task, &body) {
            Ok(pr) => {
                let opened = pr.as_ref().is_some_and(|pr| pr.by_factory)
                    && task.pr.as_ref().map(|own| own.number) != pr.as_ref().map(|pr| pr.number);
                self.with_task(factory_id, id, |task| {
                    // The Factory's own pull request found again stays its own.
                    let own = task.pr.as_ref().is_some_and(|own| {
                        own.by_factory && pr.as_ref().is_some_and(|pr| pr.number == own.number)
                    });
                    task.pr = pr.map(|pr| PullRequest {
                        by_factory: pr.by_factory || own,
                        ..pr
                    });
                    task.writes.insert("pr".into());
                    task.writes.remove(PUBLISH_PENDING);
                });
                self.publish_retry.remove(&key);
                self.publish_refusals.remove(&key);
                self.record(
                    factory_id,
                    Some(id),
                    "github.published",
                    json!({"opened": opened}),
                );
                let _ = self.begin_verification(factory_id, id);
            }
            Err(failure) => {
                self.external_failure(factory_id, Some(id), &failure);
                // An environment signal in between breaks the run of refusals.
                let refusals = if failure.signal.is_none() {
                    let n = self.publish_refusals.entry(key.clone()).or_default();
                    *n += 1;
                    *n
                } else {
                    self.publish_refusals.remove(&key);
                    0
                };
                if refusals >= 2 {
                    // A protected branch or a hook refuses the same way each
                    // time: a person fixes the cause and retries (rule 10).
                    self.publish_refusals.remove(&key);
                    self.publish_retry.remove(&key);
                    let detail = judgment::cut(&failure.detail, 300);
                    self.with_task(factory_id, id, |t| {
                        t.writes.remove(PUBLISH_PENDING);
                        t.stop = Some(StopReason::PublishRefused);
                        t.stop_detail = Some(detail);
                    });
                    self.set_state(factory_id, id, TaskState::Stopped);
                    self.with_task(factory_id, id, |t| {
                        t.stop = Some(StopReason::PublishRefused);
                    });
                } else {
                    self.publish_retry.insert(key, now + PUBLISH_RETRY_MS);
                }
            }
        }
    }

    /// Starts the checks and the Task-stage verification of a reported Task.
    fn begin_verification(&mut self, factory_id: &str, id: &str) -> Reply {
        let factory = self
            .factories
            .get(factory_id)
            .cloned()
            .ok_or_else(|| refuse("factory_not_found", "?"))?;
        self.start_checks(factory_id, id);
        if !factory.config.verification.configured() {
            // No verification: straight to merge waiting (B2, B35).
            self.advance_merge(factory_id, id);
            return Ok(
                json!({"message": "검증 없음: 머지 대기로 갑니다", "state": "merge_waiting"}),
            );
        }
        self.start_verification(factory_id, id, AttemptStage::Task);
        Ok(
            json!({"message": "검증 중: 결과는 편지로 갑니다. 지금은 차례를 끝내세요.", "state": "verifying"}),
        )
    }

    fn start_checks(&mut self, factory: &str, id: &str) {
        let Some(task) = self.task(factory, id).cloned() else {
            return;
        };
        let Some(f) = self.factories.get(factory).cloned() else {
            return;
        };
        // A paused Factory asks its AI nothing, and a check it cannot ask yet
        // is not a failed one: it runs when the Factory resumes (D-48, D-49).
        if f.paused {
            self.with_task(factory, id, |task| {
                task.writes.insert(CHECKS_DEFERRED.to_owned());
            });
            self.record(factory, Some(id), "checks.deferred", json!({}));
            return;
        }
        self.with_task(factory, id, |task| {
            task.writes.remove(CHECKS_DEFERRED);
        });
        // A diff that cannot be read gives the checks nothing to judge; that
        // is a check that cannot run, never a pass (B68).
        let diff = match self.ports.merge.diff_text(&f, &task) {
            Ok(diff) => diff,
            Err(failure) => {
                return self.check_failed(factory, id, &format!("diff: {}", failure.detail));
            }
        };
        let mut count = 0;
        let decisions = task.decisions.iter().map(|d| d.text.clone()).collect();
        let attempt = task.attempts.len();
        let drift = Judgment {
            id: format!("{factory}:{id}:drift:{attempt}"),
            factory: factory.to_owned(),
            task: Some(id.to_owned()),
            priority: Priority::Factory,
            input: JudgmentInput::Drift {
                card: task.card.clone(),
                diff: diff.clone(),
                decisions,
            },
            ai: None,
        };
        match self.submit_judgment(drift.clone()) {
            Ok(()) => {
                self.judgments.insert(
                    drift.id,
                    (factory.to_owned(), Some(id.to_owned()), Purpose::Drift),
                );
                count += 1;
            }
            Err(failure) => self.check_failed(factory, id, &format!("submit: {}", failure.stage)),
        }
        for (index, check) in f
            .config
            .checks
            .iter()
            .filter(|c| c.at == CheckPoint::AfterDone)
            .enumerate()
        {
            let judgment = Judgment {
                id: format!("{factory}:{id}:check:{index}:{attempt}"),
                factory: factory.to_owned(),
                task: Some(id.to_owned()),
                priority: Priority::Factory,
                input: JudgmentInput::Check {
                    instruction: check.instruction.clone(),
                    card: task.card.clone(),
                    diff: Some(diff.clone()),
                },
                ai: None,
            };
            match self.submit_judgment(judgment.clone()) {
                Ok(()) => {
                    self.judgments.insert(
                        judgment.id,
                        (factory.to_owned(), Some(id.to_owned()), Purpose::Check),
                    );
                    count += 1;
                }
                Err(failure) => {
                    self.check_failed(factory, id, &format!("submit: {}", failure.stage))
                }
            }
        }
        self.checks_running
            .insert((factory.to_owned(), id.to_owned()), count);
    }

    /// A check that could not run is never skipped: merge waits for a person (B68).
    fn check_failed(&mut self, factory: &str, id: &str, reason: &str) {
        self.with_task(factory, id, |task| {
            if !task.gates.contains(&Gate::CheckFailed) {
                task.gates.push(Gate::CheckFailed);
            }
        });
        self.record(factory, Some(id), "check.failed", json!({"reason": reason}));
    }

    /// `holds_merge` is true for the drift and after-done checks; a
    /// periodic check on a running Task only asks and flags (B67).
    fn apply_finding(&mut self, factory: &str, id: &str, value: &Value, holds_merge: bool) {
        let finding = match judgment::parse_finding(value) {
            Ok(finding) => finding,
            Err(reason) if holds_merge => return self.check_failed(factory, id, &reason),
            Err(_) => return,
        };
        // A failing answer with nothing to ask or flag still holds the merge
        // for a person.
        if holds_merge && !finding.pass && finding.questions.is_empty() && finding.flags.is_empty()
        {
            return self.check_failed(factory, id, "failed with nothing to ask");
        }
        let now = self.now();
        let deadline = self
            .factories
            .get(factory)
            .map_or(24 * HOUR_MS, |f| f.config.question_deadline_ms);
        for question in &finding.questions {
            // The Task stays verifying; the worker is not woken (D-44).
            self.add_question(
                factory,
                id,
                QuestionOrigin::Check,
                QuestionKind::Default,
                &question.text,
                &question.suggestion,
                question.default_action.clone(),
                Some(now + deadline),
                Vec::new(),
                None,
            );
        }
        self.with_task(factory, id, |task| {
            for flag in &finding.flags {
                if flag.to_lowercase().contains("breaking") || flag.contains("contract") {
                    task.breaking = true;
                }
                task.flags.push(flag.clone());
            }
        });
    }

    // ------------------------------------------------------------ verification

    fn start_verification(&mut self, factory_id: &str, id: &str, stage: AttemptStage) {
        let Some(factory) = self.factories.get(factory_id).cloned() else {
            return;
        };
        let Some(task) = self.task(factory_id, id).cloned() else {
            return;
        };
        let started = match stage {
            AttemptStage::Task => self.ports.verifier.start(&factory, &task),
            AttemptStage::PreMerge => self.ports.verifier.start_premerge(&factory, &task),
        };
        let now = self.now();
        let number = task.failures + 1;
        match started {
            Ok(run) => {
                self.with_task(factory_id, id, |task| {
                    task.attempts.push(Attempt {
                        number,
                        commit: run.commit.clone(),
                        started_at: now,
                        stage,
                        outcome: None,
                        log: run.log.clone(),
                    });
                });
                self.record(
                    factory_id,
                    Some(id),
                    "verify.started",
                    json!({"stage": format!("{stage:?}"), "attempt": number}),
                );
                self.verifying.insert(
                    (factory_id.to_owned(), id.to_owned()),
                    VerifyState { run, stage },
                );
            }
            Err(failure) => self.verification_environment(factory_id, id, &failure, "start"),
        }
    }

    fn restart_verification(&mut self, factory: &str, id: &str) {
        let stage = self
            .task(factory, id)
            .and_then(|task| task.attempts.last())
            .filter(|attempt| attempt.outcome.is_none())
            .map(|attempt| attempt.stage);
        if let Some(stage) = stage {
            self.with_task(factory, id, |task| {
                task.attempts.pop();
            });
            self.start_verification(factory, id, stage);
        }
        // A report still to be published starts its checks once pushed.
        if self
            .task(factory, id)
            .is_some_and(|task| !task.writes.contains(PUBLISH_PENDING))
        {
            self.start_checks(factory, id);
        }
    }

    fn cancel_verification(&mut self, factory: &str, id: &str) {
        if let Some(state) = self.verifying.remove(&(factory.to_owned(), id.to_owned())) {
            self.ports.verifier.cancel(&state.run);
        }
    }

    fn poll_verifications(&mut self) {
        let keys: Vec<(String, String)> = self.verifying.keys().cloned().collect();
        for (factory_id, id) in keys {
            let Some(factory) = self.factories.get(&factory_id).cloned() else {
                continue;
            };
            let Some(state) = self.verifying.get(&(factory_id.clone(), id.clone())) else {
                continue;
            };
            let stage = state.stage;
            let poll = self.ports.verifier.poll(&factory, &state.run);
            let outcome = match poll {
                VerifyPoll::Pending => continue,
                VerifyPoll::Passed => AttemptOutcome::Passed,
                VerifyPoll::Failed { check, link } => AttemptOutcome::Failed { check, link },
                VerifyPoll::Environment { signal, check } => AttemptOutcome::Environment {
                    signal: signal.as_str().into(),
                    check,
                },
            };
            self.verifying.remove(&(factory_id.clone(), id.clone()));
            self.with_task(&factory_id, &id, |task| {
                if let Some(attempt) = task.attempts.last_mut() {
                    attempt.outcome = Some(outcome.clone());
                }
            });
            self.record(
                &factory_id,
                Some(&id),
                "verify.finished",
                json!({"stage": format!("{stage:?}"), "outcome": outcome}),
            );
            match outcome {
                AttemptOutcome::Passed => {
                    self.with_task(&factory_id, &id, |task| task.environment_failures = 0);
                    if stage == AttemptStage::PreMerge {
                        self.with_task(&factory_id, &id, |task| {
                            task.writes.insert("premerge_passed".into());
                        });
                    }
                    self.advance_merge(&factory_id, &id);
                }
                AttemptOutcome::Failed { check, link } => {
                    self.verification_failed(&factory_id, &id, &check, &link)
                }
                AttemptOutcome::Environment { signal, check } => {
                    let signal = parse_signal(&signal).unwrap_or(EnvSignal::Network);
                    let failure = Failure::environment(&check, signal, "verification");
                    self.verification_environment(&factory_id, &id, &failure, &check);
                }
            }
        }
    }

    /// One failure is a failure; nothing runs again (D-23, B37).
    fn verification_failed(&mut self, factory: &str, id: &str, check: &str, link: &str) {
        let limit = self
            .factories
            .get(factory)
            .map_or(3, |f| f.config.verify_failure_limit);
        let now = self.now();
        self.with_task(factory, id, |task| {
            task.failures += 1;
            task.environment_failures = 0;
            task.writes.remove("premerge_passed");
        });
        let failures = self.task(factory, id).map_or(0, |t| t.failures);
        self.cascade_note(factory, id, check, now);
        if self.reclassified_by_cascade(factory, id) {
            return;
        }
        if failures >= limit {
            let last = self
                .task(factory, id)
                .and_then(|t| {
                    t.decisions
                        .iter()
                        .rev()
                        .find(|d| d.text.starts_with("done:"))
                        .map(|d| d.text.clone())
                })
                .unwrap_or_default();
            self.with_task(factory, id, |task| {
                task.stop = Some(StopReason::VerifyFailed)
            });
            self.set_state(factory, id, TaskState::Stopped);
            self.with_task(factory, id, |task| {
                task.stop = Some(StopReason::VerifyFailed)
            });
            self.add_question(factory, id, QuestionOrigin::Engine, QuestionKind::Action,
                &format!("검증이 {failures}번 실패했습니다: {check} ({link}). worker의 마지막 설명: {last}"),
                "retry", None, None, vec!["retry".into(), "cancel".into()], None);
            return;
        }
        self.set_state(factory, id, TaskState::Running);
        self.wake(factory, id, &format!(
            "Factory: 검증 실패 ({failures}/{limit}): {check}\n로그: {link}\n범위 안에서 고쳐서 다시 hide factory done 하세요. 범위 밖이 필요하면 hide factory propose 또는 ask 로 보고하세요."
        ));
    }

    // ------------------------------------------------------------------- merge

    /// Moves a verified Task toward merge: pre-merge checks, gates, then an
    /// auto merge or merge waiting (B38, B39, B40, B41, B42).
    fn advance_merge(&mut self, factory_id: &str, id: &str) {
        let Some(factory) = self.factories.get(factory_id).cloned() else {
            return;
        };
        let Some(task) = self.task(factory_id, id).cloned() else {
            return;
        };
        if task.state != TaskState::Verifying {
            return;
        }
        if task.writes.contains(PUBLISH_PENDING) || task.writes.contains(CHECKS_DEFERRED) {
            return;
        }
        let key = (factory_id.to_owned(), id.to_owned());
        // A merge asked to wait is not read again before its time (B39).
        if let Some((at, since)) = self.merge_retry.get(&key).copied() {
            if at > self.now() {
                return;
            }
            // GitHub answered a merge without naming its commit: only read
            // whether it merged. A merged one landed whatever a gate says
            // now; one that did not merge goes through every check again.
            if since.is_some() {
                match self.ports.merge.merged_commit(&factory, &task) {
                    Ok(Some(sha)) => {
                        self.landed(factory_id, id, &sha);
                        return;
                    }
                    Ok(None) => {
                        self.merge_retry.remove(&key);
                    }
                    Err(failure) => {
                        self.merge_failed(factory_id, id, &failure);
                        return;
                    }
                }
            }
        }
        if self.verifying.contains_key(&key) {
            return;
        }
        if self.checks_running.get(&key).is_some_and(|n| *n > 0) {
            return;
        }
        let verified = match &factory.config.verification {
            Verification::None => true,
            _ => task
                .attempts
                .iter()
                .rev()
                .find(|a| a.stage == AttemptStage::Task)
                .is_some_and(|a| a.outcome == Some(AttemptOutcome::Passed)),
        };
        if !verified {
            return;
        }
        // A paused Factory reads nothing more toward a merge (D-48): the
        // pre-merge check and its verification wait for the resume.
        if factory.paused {
            return;
        }
        let now = self.now();
        // Questions with a default wait only here, until answer or deadline (B26).
        let waiting_question = task.open_questions().any(|q| {
            matches!(
                q.kind,
                QuestionKind::Default | QuestionKind::ScopeChange { .. }
            ) && q.deadline.is_none_or(|d| d > now)
        });
        if waiting_question {
            return;
        }
        let mode = task.merge_mode(&factory);
        if factory.main.broken && mode == MergeMode::Auto {
            // Auto merge is stopped until main is green again (B44, D-47):
            // a person's gate still shows now, and nothing is read per tick.
            let mut gates = self.person_gates(&factory, &task, mode);
            if waits_on_answer(&task) {
                gates.push(Gate::OpenQuestion);
            }
            if !gates.is_empty() {
                self.with_task(factory_id, id, |task| task.gates = gates.clone());
                self.set_state(factory_id, id, TaskState::MergeWaiting);
            }
            return;
        }
        // Merge-tree and the quick check, in seconds (B38).
        if factory.config.verification.configured() || factory.source == SourceKind::Local {
            match self.ports.merge.premerge(&factory, &task) {
                Ok(PreMerge::Clean) => {}
                Ok(PreMerge::Conflict { files }) => {
                    self.send_to_rebase(factory_id, id, &files);
                    return;
                }
                Ok(PreMerge::QuickCheckFailed { check }) => {
                    self.verification_failed(factory_id, id, &check, "quick check");
                    return;
                }
                Ok(PreMerge::RiskPath { paths }) => {
                    self.with_task(factory_id, id, |task| {
                        if !task.gates.contains(&Gate::RiskPath) {
                            task.gates.push(Gate::RiskPath);
                        }
                    });
                    self.record(
                        factory_id,
                        Some(id),
                        "merge.risk_path",
                        json!({"paths": paths.len()}),
                    );
                }
                Err(failure) => {
                    self.external_failure(factory_id, Some(id), &failure);
                    return;
                }
            }
        }
        // Verify factories run the bundle once on the latest main merged in.
        if matches!(factory.config.verification, Verification::Commands { .. })
            && !task.writes.contains("premerge_passed")
        {
            self.start_verification(factory_id, id, AttemptStage::PreMerge);
            return;
        }
        let task = self.task(factory_id, id).cloned().unwrap_or(task);
        let mut gates: Vec<Gate> = task
            .gates
            .iter()
            .copied()
            .filter(|g| matches!(g, Gate::RiskPath | Gate::CheckFailed))
            .collect();
        gates.extend(self.person_gates(&factory, &task, mode));
        if task.autonomy.is_some() {
            let lines = self
                .ports
                .merge
                .diff_lines(&factory, &task)
                .unwrap_or(u32::MAX);
            if lines > factory.config.autonomy_diff_limit {
                gates.push(Gate::AutonomyDiff);
            }
        }
        if waits_on_answer(&task) {
            gates.push(Gate::OpenQuestion);
        }
        gates.dedup();
        if !gates.is_empty() {
            let risk_only = gates == [Gate::RiskPath];
            self.with_task(factory_id, id, |task| task.gates = gates.clone());
            self.set_state(factory_id, id, TaskState::MergeWaiting);
            // In 맡김 the Observer may approve a risk path that is the only
            // gate of a verified Task (D-21); a person still can first.
            if risk_only && !factory.main.broken {
                self.ask_risk_merge(factory_id, id);
            }
            return;
        }
        if factory.main.broken {
            // A manual Task always has its gate; nothing merges on red.
            return;
        }
        let _ = self.merge_now(factory_id, id);
    }

    /// A conflict with main sends the worker to rebase; it is not a
    /// verification failure (B40).
    fn send_to_rebase(&mut self, factory_id: &str, id: &str, files: &[String]) {
        self.record(
            factory_id,
            Some(id),
            "merge.conflict",
            json!({"files": files.len()}),
        );
        self.with_task(factory_id, id, |task| task.gates.clear());
        self.set_state(factory_id, id, TaskState::Running);
        self.wake(factory_id, id, &format!(
            "Factory: 최신 main과 충돌합니다 ({}). main 위로 rebase한 뒤 다시 hide factory done 하세요. 이 일은 검증 실패로 세지 않습니다.",
            files.join(", ")
        ));
    }

    /// The gates a person decides that need no git read (D-25).
    fn person_gates(&self, factory: &Factory, task: &Task, mode: MergeMode) -> Vec<Gate> {
        let mut gates = Vec::new();
        if task.human.review_directly {
            gates.push(Gate::ReviewDirectly);
        }
        if task.scope_approved {
            gates.push(Gate::ApprovedScopeChange);
        }
        if task.breaking {
            gates.push(Gate::BreakingChange);
        }
        if !factory.config.verification.configured() {
            gates.push(Gate::NoVerification);
        }
        if mode == MergeMode::Manual {
            gates.push(Gate::ManualMode);
        }
        gates
    }

    /// Merges now. A refusal that is neither the environment's nor a merge
    /// GitHub has not finished naming waits for a person with its reason
    /// instead of being retried every tick.
    fn merge_now(&mut self, factory_id: &str, id: &str) -> Result<String, Failure> {
        let unknown = || Failure::task("merge", "unknown Task");
        let factory = self
            .factories
            .get(factory_id)
            .cloned()
            .ok_or_else(unknown)?;
        let task = self.task(factory_id, id).cloned().ok_or_else(unknown)?;
        if factory.source == SourceKind::Local {
            match self.ports.merge.main_dirty(&factory) {
                Ok(true) => {
                    self.with_task(factory_id, id, |task| {
                        if !task.gates.contains(&Gate::DirtyMain) {
                            task.gates.push(Gate::DirtyMain);
                        }
                    });
                    self.set_state(factory_id, id, TaskState::MergeWaiting);
                    return Err(Failure::task("merge", "main_dirty"));
                }
                Ok(false) => {}
                Err(failure) => {
                    self.external_failure(factory_id, Some(id), &failure);
                    return Err(failure);
                }
            }
        }
        match self
            .ports
            .merge
            .merge(&factory, &task, factory.config.merge_method)
        {
            Ok(sha) => {
                self.landed(factory_id, id, &sha);
                Ok(sha)
            }
            Err(failure) => {
                self.merge_failed(factory_id, id, &failure);
                Err(failure)
            }
        }
    }

    /// Records a merge that landed on main (B39, B44).
    fn landed(&mut self, factory_id: &str, id: &str, sha: &str) {
        let sha = sha.to_owned();
        let now = self.now();
        self.merge_retry
            .remove(&(factory_id.to_owned(), id.to_owned()));
        self.with_task(factory_id, id, |task| {
            task.merge_sha = Some(sha.clone());
            task.writes.insert("merge".into());
            task.gates.clear();
        });
        if let Some(f) = self.factories.get_mut(factory_id) {
            f.main.merges_since_green.push(LandedMerge {
                task: id.to_owned(),
                sha: sha.clone(),
                at: now,
            });
        }
        self.save_factory(factory_id);
        self.main_seen.insert(factory_id.to_owned(), sha.clone());
        self.record(factory_id, Some(id), "merge.done", json!({"sha": sha}));
        self.set_state(factory_id, id, TaskState::Landed);
        self.check_landed(factory_id, id);
    }

    /// A merge that did not land: asked again at its time when it was asked
    /// to wait, or held for a person when it was refused.
    fn merge_failed(&mut self, factory_id: &str, id: &str, failure: &Failure) {
        self.external_failure(factory_id, Some(id), failure);
        let now = self.now();
        let key = (factory_id.to_owned(), id.to_owned());
        // Asked to wait (a merge commit not named yet, or GitHub or
        // the network down): tried again at its time, not per tick.
        let backoff = self
            .github_backoff
            .get(factory_id)
            .map(|(_, until)| *until)
            .filter(|until| *until > now)
            .filter(|_| {
                matches!(
                    failure.signal,
                    Some(EnvSignal::GithubRateLimit | EnvSignal::GithubServer | EnvSignal::Network)
                )
            });
        let retry_at = match (failure.again_in_ms, failure.signal) {
            (Some(wait), _) => Some(now + wait.max(1)),
            (None, Some(_)) => Some(backoff.unwrap_or(now + PUBLISH_RETRY_MS)),
            (None, None) => None,
        };
        let since = self.merge_retry.get(&key).and_then(|(_, since)| *since);
        // A signal while reading an unnamed merge keeps it unnamed: the
        // merge may already have happened, and the 10 minutes still count
        // from GitHub's first answer.
        let unnamed = (failure.again_in_ms.is_some()
            || (failure.signal.is_some() && since.is_some()))
        .then(|| since.unwrap_or(now));
        if unnamed.is_some_and(|since| now.saturating_sub(since) >= MERGE_UNNAMED_LIMIT_MS) {
            // GitHub never named the commit: a person looks (rule 15).
            self.merge_retry.remove(&key);
            self.with_task(factory_id, id, |task| {
                if !task.gates.contains(&Gate::MergeRefused) {
                    task.gates.push(Gate::MergeRefused);
                }
            });
            self.record(
                factory_id,
                Some(id),
                "merge.refused",
                json!({"stage": failure.stage, "detail": "the merge commit was not named in 10 minutes"}),
            );
            self.set_state(factory_id, id, TaskState::MergeWaiting);
            return;
        }
        match retry_at {
            Some(at) => {
                self.merge_retry.insert(key, (at, unnamed));
            }
            None => {
                self.merge_retry.remove(&key);
            }
        }
        if failure.signal.is_none() && failure.again_in_ms.is_none() {
            self.with_task(factory_id, id, |task| {
                if !task.gates.contains(&Gate::MergeRefused) {
                    task.gates.push(Gate::MergeRefused);
                }
            });
            self.record(
                factory_id,
                Some(id),
                "merge.refused",
                json!({"stage": failure.stage, "detail": judgment::cut(&failure.detail, 300)}),
            );
            self.set_state(factory_id, id, TaskState::MergeWaiting);
        }
    }

    fn manual_merge(&mut self, _role: &Role, factory_id: &str, id: &str) -> Reply {
        let factory = self
            .factories
            .get(factory_id)
            .cloned()
            .ok_or_else(|| refuse("factory_not_found", "?"))?;
        if factory.source == SourceKind::Local
            && matches!(self.ports.merge.main_dirty(&factory), Ok(true))
        {
            return Err(refuse(
                "main_dirty",
                "Commit or stash the changes in the main checkout, then merge",
            ));
        }
        // Main may have moved since the Task waited: merge-tree and the
        // quick check run again right before a person's merge (B38, B40).
        if let Some(task) = self.task(factory_id, id).cloned() {
            match self.ports.merge.premerge(&factory, &task) {
                Ok(PreMerge::Clean) | Ok(PreMerge::RiskPath { .. }) => {}
                Ok(PreMerge::Conflict { files }) => {
                    self.send_to_rebase(factory_id, id, &files);
                    return Err(refuse(
                        "merge_conflict",
                        "The Task conflicts with main; its worker rebases and reports again",
                    ));
                }
                Ok(PreMerge::QuickCheckFailed { check }) => {
                    self.verification_failed(factory_id, id, &check, "quick check");
                    return Err(refuse(
                        "quick_check_failed",
                        "The quick check failed on the latest main; its worker was told",
                    ));
                }
                Err(failure) => {
                    self.external_failure(factory_id, Some(id), &failure);
                    return Err(refuse(
                        "merge_failed",
                        format!("{}: {}", failure.stage, judgment::cut(&failure.detail, 300)),
                    ));
                }
            }
        }
        match self.merge_now(factory_id, id) {
            Ok(sha) => Ok(
                json!({"message": "merged", "sha": sha, "task": self.task_answer(factory_id, id, "")["task"]}),
            ),
            Err(failure) => Err(refuse(
                "merge_failed",
                format!("{}: {}", failure.stage, judgment::cut(&failure.detail, 300)),
            )),
        }
    }

    fn check_landed(&mut self, factory_id: &str, id: &str) {
        let Some(factory) = self.factories.get(factory_id).cloned() else {
            return;
        };
        let Some(task) = self.task(factory_id, id).cloned() else {
            return;
        };
        let Some(sha) = task.merge_sha.clone() else {
            return;
        };
        match self.ports.merge.main_check(&factory, &sha) {
            Ok(MainCheck::Green) | Ok(MainCheck::None) => self.finish(factory_id, id, &sha),
            Ok(MainCheck::Pending) => {}
            Ok(MainCheck::Red { link }) => self.main_broken(factory_id, &sha, &link),
            Err(failure) => self.external_failure(factory_id, Some(id), &failure),
        }
    }

    fn finish(&mut self, factory_id: &str, id: &str, sha: &str) {
        self.set_state(factory_id, id, TaskState::Done);
        if let Some(f) = self.factories.get_mut(factory_id) {
            f.main.last_green = Some(sha.to_owned());
            if let Some(position) = f.main.merges_since_green.iter().position(|m| m.sha == sha) {
                f.main.merges_since_green.drain(..=position);
            }
        }
        self.save_factory(factory_id);
        // Worker tab and worktree go once main is green (B43).
        self.release_worker(factory_id, id);
        // A finished Task is a watch event (D-32).
        self.watch_due(factory_id, true);
    }

    /// Stops a worker; a refusal is recorded against its Task.
    fn stop_worker(&mut self, factory_id: &str, id: &str, worker: &WorkerRef) {
        // A worker that ends frees disk and memory: read them again (B57).
        self.hold_checked_at = None;
        if let Err(failure) = self.ports.workers.stop(worker) {
            self.external_failure(factory_id, Some(id), &failure);
        }
    }

    /// An outside pull request takes the Task over (D-27, B50): a worker is
    /// stopped and its worktree kept for the keep period from now, so the
    /// work it had not reported survives until then.
    fn follow_outside(&mut self, factory_id: &str, task: &Task) {
        // A Task already taken keeps the keep period of its first takeover.
        if task.state == TaskState::Outside && task.cancelled_at.is_some() {
            return;
        }
        let now = self.now();
        self.with_task(factory_id, &task.id, |t| {
            t.cancelled_at = Some(now);
            t.cancelled_from = Some(t.state);
        });
        let Some(worker) = &task.worker else { return };
        if task.purged {
            return;
        }
        self.stop_worker(factory_id, &task.id, worker);
        self.with_task(factory_id, &task.id, |t| {
            if let Some(w) = &mut t.worker {
                w.asleep = true;
            }
        });
    }

    /// A finished Task's worker ends and its worktree goes; the branch stays
    /// with the merge. A failure is logged and the disk recovery retries it.
    fn release_worker(&mut self, factory_id: &str, id: &str) {
        let Some(worker) = self.task(factory_id, id).and_then(|t| t.worker.clone()) else {
            return;
        };
        self.stop_worker(factory_id, id, &worker);
        match self
            .ports
            .workers
            .remove_worktree(&worker, Removal::Finished)
        {
            Ok(()) => {
                self.with_task(factory_id, id, |t| t.purged = true);
            }
            Err(failure) => self.cleanup_failed(factory_id, id, &failure),
        }
    }

    fn cleanup_failed(&mut self, factory_id: &str, id: &str, failure: &Failure) {
        self.record(
            factory_id,
            Some(id),
            "cleanup.failed",
            json!({"stage": failure.stage, "detail": failure.detail}),
        );
        // A worktree that refuses to go (leftovers in a finished Task's) is
        // a person's to look at; the environment's failures are retried.
        if failure.signal.is_none()
            && let Some(worktree) = self
                .task(factory_id, id)
                .and_then(|t| t.worker.as_ref())
                .map(|w| w.worktree.clone())
        {
            self.once_notice(
                factory_id,
                id,
                &format!(
                    "worktree를 지우지 못했습니다: {worktree} ({}). 남은 변경을 확인한 뒤 직접 지우세요.",
                    judgment::cut(&failure.detail, 200)
                ),
            );
        }
    }

    // ------------------------------------------------------------- main break

    fn main_broken(&mut self, factory_id: &str, sha: &str, link: &str) {
        let Some(factory) = self.factories.get(factory_id).cloned() else {
            return;
        };
        if factory.main.broken && factory.main.reason.as_deref() == Some(sha) {
            return;
        }
        let by_factory = factory.main.merges_since_green.iter().any(|m| m.sha == sha);
        if let Some(f) = self.factories.get_mut(factory_id) {
            f.main.broken = true;
            f.main.reason = Some(sha.to_owned());
        }
        self.save_factory(factory_id);
        self.record(
            factory_id,
            None,
            "main.broken",
            json!({"sha": sha, "by_factory": by_factory}),
        );
        // The notice rides on the last merged Task, or the draft fix Task.
        if !by_factory {
            // Outside push: no revert; a fix Task draft and a notice (B47).
            let card = Card {
                title: format!("main 고치기 ({})", &sha[..sha.len().min(8)]),
                goal: format!("Factory 밖 push 뒤 main 검증이 실패했습니다: {link}"),
                criteria: vec!["main 검증이 다시 통과한다".into()],
                ..Card::default()
            };
            // The draft waits for a person's confirmation like a labelled
            // issue's card (D-48).
            let fix = self.new_task(factory_id, card, None, None);
            self.add_question(
                factory_id,
                &fix,
                QuestionOrigin::Engine,
                QuestionKind::ConfirmCard,
                &format!("main 깨짐: Factory 밖 push({sha})로 검증이 실패해 자동 머지를 멈췄습니다. 수정 Task 초안을 확인해 주세요. {link}"),
                "confirm",
                None,
                None,
                vec!["confirm".into(), "cancel".into()],
                None,
            );
            let now = self.now();
            self.request_review(factory_id, &fix, now);
            self.watch_due(factory_id, true);
            return;
        }
        let anchor = factory
            .main
            .merges_since_green
            .last()
            .map(|m| m.task.clone())
            .unwrap_or_default();
        if !anchor.is_empty() {
            self.notice(factory_id, &anchor, &format!("main 깨짐: Factory 머지 뒤 main 검증이 실패했습니다. 자동 머지를 멈추고 원인 머지를 찾습니다. {link}"));
        }
        self.set_recovery(factory_id, Some(RevertPhase::Bisecting { index: 0 }));
        self.watch_due(factory_id, true);
        self.drive_revert(factory_id);
    }

    /// Finds the first failing merge since the last green and reverts it
    /// alone (D-26, B45); a stage that cannot decide stops for a person (B48).
    fn drive_revert(&mut self, factory_id: &str) {
        let Some(factory) = self.factories.get(factory_id).cloned() else {
            return;
        };
        if factory.main.needs_person {
            return;
        }
        let Some(phase) = self.reverts.get(factory_id).cloned() else {
            return;
        };
        match phase {
            RevertPhase::Bisecting { index } => {
                let merges = factory.main.merges_since_green.clone();
                if merges.len() == 1 {
                    let task = merges[0].task.clone();
                    return self.start_revert(factory_id, &task);
                }
                let mut index = index;
                while index < merges.len() {
                    let merge = &merges[index];
                    match self.ports.merge.main_check(&factory, &merge.sha) {
                        Ok(MainCheck::Green) => index += 1,
                        Ok(MainCheck::Red { .. }) => {
                            let task = merge.task.clone();
                            return self.start_revert(factory_id, &task);
                        }
                        Ok(MainCheck::Pending) => {
                            // A skipped or cancelled run is asked again once.
                            let key = format!("rerun:{}", merge.sha);
                            let asked = self
                                .task(factory_id, &merge.task)
                                .is_some_and(|t| t.writes.contains(&key));
                            if !asked {
                                if let Err(failure) =
                                    self.ports.merge.rerun_main(&factory, &merge.sha)
                                {
                                    self.external_failure(
                                        factory_id,
                                        Some(&merge.task.clone()),
                                        &failure,
                                    );
                                }
                                let task = merge.task.clone();
                                self.with_task(factory_id, &task, |t| {
                                    t.writes.insert(key);
                                });
                            }
                            self.set_recovery(factory_id, Some(RevertPhase::Bisecting { index }));
                            return;
                        }
                        Ok(MainCheck::None) | Err(_) => break,
                    }
                }
                self.revert_needs_person(
                    factory_id,
                    "원인 머지를 하나로 정하지 못했습니다",
                    &merges,
                );
            }
            RevertPhase::Reverting { revert, task } => {
                match self.ports.merge.revert_check(&factory, &revert) {
                    Ok(MainCheck::Pending) => {}
                    Ok(MainCheck::Green) | Ok(MainCheck::None) => {
                        match self.ports.merge.merge_revert(&factory, &revert) {
                            Ok(sha) => {
                                self.set_recovery(factory_id, None);
                                self.record(
                                    factory_id,
                                    Some(&task),
                                    "main.reverted",
                                    json!({"sha": sha}),
                                );
                                if let Some(f) = self.factories.get_mut(factory_id) {
                                    f.main.merges_since_green.retain(|m| m.task != task);
                                }
                                self.save_factory(factory_id);
                                self.main_seen.remove(factory_id);
                                // The original Task lands again on the latest main (B46).
                                self.with_task(factory_id, &task, |t| {
                                    t.merge_sha = None;
                                    t.writes.remove("merge");
                                    t.writes.remove("premerge_passed");
                                    t.writes.insert(format!("reverted:{}", revert.sha));
                                    // A new pull request carries it again.
                                    t.pr = None;
                                });
                                self.set_state(factory_id, &task, TaskState::Relanding);
                            }
                            Err(failure) => {
                                self.external_failure(factory_id, Some(&task), &failure);
                                let merges = factory.main.merges_since_green.clone();
                                self.revert_needs_person(
                                    factory_id,
                                    "revert를 머지하지 못했습니다",
                                    &merges,
                                );
                            }
                        }
                    }
                    Ok(MainCheck::Red { .. }) | Err(_) => {
                        let merges = factory.main.merges_since_green.clone();
                        self.revert_needs_person(
                            factory_id,
                            "revert의 검증이 실패했습니다",
                            &merges,
                        );
                    }
                }
            }
        }
    }

    fn start_revert(&mut self, factory_id: &str, task: &str) {
        let Some(factory) = self.factories.get(factory_id).cloned() else {
            return;
        };
        let Some(sha) = self
            .task(factory_id, task)
            .and_then(|t| t.merge_sha.clone())
        else {
            let merges = factory.main.merges_since_green.clone();
            return self.revert_needs_person(factory_id, "되돌릴 머지를 찾지 못했습니다", &merges);
        };
        let Some(target) = self.task(factory_id, task).cloned() else {
            return;
        };
        match self.ports.merge.revert(&factory, &target, &sha) {
            Ok(revert) => {
                self.record(
                    factory_id,
                    Some(task),
                    "main.revert_opened",
                    json!({"sha": sha}),
                );
                self.with_task(factory_id, task, |t| {
                    t.writes.insert(format!("revert:{sha}"));
                });
                self.set_recovery(
                    factory_id,
                    Some(RevertPhase::Reverting {
                        revert,
                        task: task.to_owned(),
                    }),
                );
                self.drive_revert(factory_id);
            }
            Err(failure) => {
                self.external_failure(factory_id, Some(task), &failure);
                let merges = factory.main.merges_since_green.clone();
                self.revert_needs_person(factory_id, "revert를 만들지 못했습니다", &merges);
            }
        }
    }

    /// The recovery phase lives in memory, and `main.recovering` in the store
    /// says one was running, so a restart can hand it to a person (B72).
    fn set_recovery(&mut self, factory_id: &str, phase: Option<RevertPhase>) {
        let running = phase.is_some();
        match phase {
            Some(phase) => {
                self.reverts.insert(factory_id.to_owned(), phase);
            }
            None => {
                self.reverts.remove(factory_id);
            }
        }
        if let Some(f) = self.factories.get_mut(factory_id)
            && f.main.recovering != running
        {
            f.main.recovering = running;
            self.save_factory(factory_id);
        }
    }

    fn revert_needs_person(&mut self, factory_id: &str, why: &str, merges: &[LandedMerge]) {
        if let Some(f) = self.factories.get_mut(factory_id) {
            f.main.needs_person = true;
        }
        self.save_factory(factory_id);
        self.set_recovery(factory_id, None);
        let anchor = merges.last().map(|m| m.task.clone());
        let Some(anchor) = anchor else { return };
        let mut choices = vec!["retry-revert".to_owned()];
        choices.extend(merges.iter().map(|m| format!("revert {}", m.task)));
        choices.push("resume-auto".into());
        let list: Vec<String> = merges
            .iter()
            .map(|m| format!("{} {}", m.task, &m.sha[..m.sha.len().min(8)]))
            .collect();
        self.add_question(
            factory_id,
            &anchor,
            QuestionOrigin::Engine,
            QuestionKind::Action,
            &format!(
                "main 깨짐 복구를 멈췄습니다: {why}. 후보 머지: {}",
                list.join(", ")
            ),
            "retry-revert",
            None,
            None,
            choices,
            None,
        );
    }

    // ----------------------------------------------------------------- control

    fn put_to_sleep(&mut self, factory: &str, id: &str) {
        if let Some(worker) = self.task(factory, id).and_then(|t| t.worker.clone()) {
            match self.ports.workers.sleep(&worker) {
                Ok(()) => {
                    self.with_task(factory, id, |task| {
                        if let Some(worker) = &mut task.worker {
                            worker.asleep = true;
                        }
                    });
                }
                Err(failure) => self.record(
                    factory,
                    Some(id),
                    "worker.sleep_failed",
                    json!({"stage": failure.stage}),
                ),
            }
        }
    }

    fn wake(&mut self, factory: &str, id: &str, body: &str) {
        let Some(worker) = self.task(factory, id).and_then(|t| t.worker.clone()) else {
            return;
        };
        self.keep(factory, Some(id), "letter.out", "wake", body);
        let now = self.now();
        let result = if worker.asleep {
            self.ports.workers.wake(&worker, body)
        } else {
            self.ports
                .workers
                .message(&worker, &format!("factory-{id}-{}", self.now()), None, body)
        };
        match result {
            Ok(()) => {
                self.with_task(factory, id, |task| {
                    if let Some(worker) = &mut task.worker {
                        worker.asleep = false;
                    }
                    task.woken_at = Some(now);
                });
            }
            Err(failure) => self.external_failure(factory, Some(id), &failure),
        }
    }

    fn reply(&mut self, factory: &str, id: &str, letter: Option<&str>, body: &str) {
        let Some(worker) = self.task(factory, id).and_then(|t| t.worker.clone()) else {
            return;
        };
        let state = self.task(factory, id).map(|t| t.state);
        if worker.asleep || state == Some(TaskState::Blocked) {
            // A blocked Task wakes when it gets a slot again.
            self.with_task(factory, id, |task| {
                task.flags
                    .push(format!("pending reply: {}", judgment::cut(body, 2000)))
            });
            return;
        }
        let intent = format!("factory-{id}-reply-{}", self.now());
        self.keep(
            factory,
            Some(id),
            "letter.out",
            letter.unwrap_or(&intent),
            body,
        );
        if let Err(failure) = self.ports.workers.message(&worker, &intent, letter, body) {
            self.external_failure(factory, Some(id), &failure);
        }
    }

    fn cancel(&mut self, factory_id: &str, id: &str) {
        let Some(task) = self.task(factory_id, id).cloned() else {
            return;
        };
        let now = self.now();
        self.cancel_verification(factory_id, id);
        if let Some(worker) = &task.worker {
            self.stop_worker(factory_id, id, worker);
        }
        if let (Some(factory), Some(pr)) = (self.factories.get(factory_id).cloned(), &task.pr)
            && pr.by_factory
            && pr.open
        {
            match self.ports.merge.close_pr(&factory, pr) {
                Ok(()) => {
                    self.with_task(factory_id, id, |task| {
                        if let Some(pr) = &mut task.pr {
                            pr.open = false;
                        }
                    });
                }
                Err(failure) => self.external_failure(factory_id, Some(id), &failure),
            }
        }
        self.with_task(factory_id, id, |task| {
            task.cancelled_at = Some(now);
            task.cancelled_from = Some(task.state);
            if let Some(worker) = &mut task.worker {
                worker.asleep = true;
            }
            // A cancelled Task asks nothing of a person, and no deadline
            // applies a default to it; a revived worker asks again.
            for question in task.questions.iter_mut().filter(|q| q.open()) {
                question.answer = Some(Answer {
                    text: "취소됨".into(),
                    chose: None,
                    relayed_by: "cancel".into(),
                    at: now,
                });
            }
        });
        self.set_state(factory_id, id, TaskState::Cancelled);
    }

    fn revive(&mut self, factory_id: &str, id: &str) -> Reply {
        let task = self
            .task(factory_id, id)
            .cloned()
            .ok_or_else(|| refuse("task_not_found", "?"))?;
        let keep = self
            .factories
            .get(factory_id)
            .map_or(7 * DAY_MS, |f| f.config.cancel_keep_ms);
        if task.purged
            || task
                .cancelled_at
                .is_some_and(|at| self.now().saturating_sub(at) > keep)
        {
            return Err(refuse("revive_expired", "Add the Task again"));
        }
        if let (Some(factory), Some(pr)) = (self.factories.get(factory_id).cloned(), &task.pr)
            && pr.by_factory
            && !pr.open
        {
            match self.ports.merge.reopen_pr(&factory, pr) {
                Ok(()) => {
                    self.with_task(factory_id, id, |task| {
                        if let Some(pr) = &mut task.pr {
                            pr.open = true;
                        }
                    });
                }
                Err(failure) => self.external_failure(factory_id, Some(id), &failure),
            }
        }
        let target = match task.cancelled_from {
            Some(TaskState::Drafting) => TaskState::Drafting,
            _ if task.issue.is_none() => TaskState::Drafting,
            _ => TaskState::Waiting,
        };
        self.with_task(factory_id, id, |task| {
            task.cancelled_at = None;
            task.cancelled_from = None;
        });
        self.set_state(factory_id, id, target);
        if target == TaskState::Drafting {
            self.maybe_ready(factory_id, id);
        }
        Ok(self.task_answer(factory_id, id, "revived with the same branch and worktree"))
    }

    fn dep(&mut self, role: &Role, task: &str, on: &str, remove: bool) -> Reply {
        let (factory, id) = self.resolve(role, task)?;
        if let Role::Worker { task: own, .. } = role
            && *own != id
        {
            return Err(refuse(
                ROLE_NOT_ALLOWED,
                "A worker changes only its own Task",
            ));
        }
        let (_, on_id) = self.resolve_within(Some(&factory), on)?;
        let current = self
            .task(&factory, &id)
            .cloned()
            .ok_or_else(|| refuse("task_not_found", "?"))?;
        if remove {
            self.allowed(&factory, &id, "dep-remove")?;
            self.with_task(&factory, &id, |task| {
                task.card.depends_on.retain(|d| *d != on_id)
            });
            self.record(
                &factory,
                Some(&id),
                "dependency.removed",
                json!({"on": on_id, "by": role.relayed_by()}),
            );
            return Ok(self.task_answer(&factory, &id, "dependency removed"));
        }
        if matches!(
            current.state,
            TaskState::Done | TaskState::Landed | TaskState::Cancelled
        ) {
            return Err(refuse("action_not_allowed_in_state", "The Task is finished")
                .with(json!({"state": current.state.label(), "allowed": Self::allowed_actions(&current)})));
        }
        let edges = dag::edges(self.tasks_of(&factory));
        if let Some(path) = dag::cycle_with(&edges, &id, &on_id) {
            return Err(
                refuse("dependency_cycle", "That dependency would close a loop")
                    .with(json!({"cycle": path})),
            );
        }
        self.with_task(&factory, &id, |task| {
            if !task.card.depends_on.contains(&on_id) {
                task.card.depends_on.push(on_id.clone());
            }
        });
        self.record(
            &factory,
            Some(&id),
            "dependency.added",
            json!({"on": on_id, "by": role.relayed_by()}),
        );
        let unmerged = self
            .task(&factory, &on_id)
            .is_some_and(|t| !t.state.merged());
        if unmerged && matches!(current.state, TaskState::Running) {
            self.put_to_sleep(&factory, &id);
            self.set_state(&factory, &id, TaskState::Blocked);
        }
        Ok(self.task_answer(&factory, &id, "dependency added"))
    }

    fn close(&mut self, role: &Role, project: Option<&str>) -> Reply {
        let _ = role;
        let factory = self.factory_id(project)?;
        let running: Vec<String> = self
            .tasks_of(&factory)
            .filter(|t| {
                !matches!(
                    t.state,
                    TaskState::Drafting
                        | TaskState::Waiting
                        | TaskState::Done
                        | TaskState::Cancelled
                )
            })
            .map(Task::display_id)
            .collect();
        if !running.is_empty() {
            return Err(refuse(
                "factory_has_running_tasks",
                "Finish, pause or cancel them first",
            )
            .with(json!({"tasks": running})));
        }
        if let Some(f) = self.factories.get_mut(&factory) {
            f.closed = true;
        }
        self.save_factory(&factory);
        self.record(&factory, None, "factory.closed", json!({}));
        Ok(
            json!({"message": "closed; hide factory init <project> --confirm brings it back with its records"}),
        )
    }

    fn config(&mut self, role: &Role, project: Option<&str>, set: Vec<(String, String)>) -> Reply {
        let factory_id = self.factory_id(project)?;
        let mut config = self
            .factories
            .get(&factory_id)
            .map(|f| f.config.clone())
            .unwrap_or_default();
        let mut machine_workers = None;
        for (key, value) in &set {
            let bad = || {
                refuse(
                    "config_invalid",
                    "Check hide factory config for the keys and values",
                )
                .with(json!({"key": key}))
            };
            let number = || value.parse::<u64>().map_err(|_| bad());
            match key.as_str() {
                "merge_mode" => {
                    config.merge_mode = match value.as_str() {
                        "auto" => MergeMode::Auto,
                        "manual" => MergeMode::Manual,
                        _ => return Err(bad()),
                    }
                }
                "merge_method" => {
                    config.merge_method = match value.as_str() {
                        "merge" => MergeMethod::Merge,
                        "squash" => MergeMethod::Squash,
                        "rebase" => MergeMethod::Rebase,
                        _ => return Err(bad()),
                    }
                }
                "verify" => {
                    let commands: Vec<String> = value
                        .split("&&&")
                        .map(str::trim)
                        .filter(|c| !c.is_empty())
                        .map(str::to_owned)
                        .collect();
                    config.verification = if commands.is_empty() {
                        Verification::None
                    } else {
                        Verification::Commands { commands }
                    };
                }
                "ci" => {
                    let checks: Vec<String> = value
                        .split(',')
                        .map(str::trim)
                        .filter(|c| !c.is_empty())
                        .map(str::to_owned)
                        .collect();
                    if checks.is_empty() {
                        return Err(refuse(
                            "ci_checks_required",
                            "Name the checks that decide: ci=<check,...>",
                        ));
                    }
                    config.verification = Verification::Ci { checks };
                }
                "no_verification" => config.verification = Verification::None,
                "quick_check" => {
                    config.quick_check = Some(value.clone()).filter(|v| !v.trim().is_empty())
                }
                "max_workers" => machine_workers = Some(number()?.clamp(1, 64) as u32),
                "question_deadline_hours" => config.question_deadline_ms = number()? * HOUR_MS,
                "stall_minutes" => config.stall_ms = number()?.max(1) * MINUTE_MS,
                "no_report_minutes" => config.no_report_ms = number()?.max(1) * MINUTE_MS,
                "archive_fold_days" => config.archive_fold_ms = number()? * DAY_MS,
                "verify_failure_limit" => config.verify_failure_limit = number()?.max(1) as u32,
                "autonomy_diff_limit" => config.autonomy_diff_limit = number()? as u32,
                "watch_interval_minutes" => config.watch_interval_ms = number()?.max(5) * MINUTE_MS,
                "watch_daily_limit" => config.watch_daily_limit = number()? as u32,
                "outside_read_minutes" => config.outside_read_ms = number()?.max(1) * MINUTE_MS,
                "cancel_keep_days" => config.cancel_keep_ms = number()? * DAY_MS,
                "done_fold_days" => config.done_fold_ms = number()? * DAY_MS,
                "new_task_limit" => config.new_task_limit = number()? as u32,
                "verify_timeout_minutes" => config.verify_timeout_ms = number()?.max(1) * MINUTE_MS,
                "disk_floor_gb" => config.disk_floor_bytes = number()? * 1024 * 1024 * 1024,
                "default_runtime" => {
                    // The first candidate's agent (D-42); a model or effort
                    // chosen for another agent does not carry over.
                    let agent = self.worker_agent(key, value)?;
                    config.default_runtime = agent;
                    if let Some(first) = config.workers.first_mut()
                        && first.agent != agent
                    {
                        *first = WorkerCandidate {
                            description: std::mem::take(&mut first.description),
                            ..WorkerCandidate::bare(agent)
                        };
                    }
                }
                "workers" => {
                    let workers: Vec<WorkerCandidate> =
                        serde_json::from_str(value).map_err(|error| {
                            refuse(
                                "config_invalid",
                                "Give workers as a JSON list of {agent, model, effort, description}",
                            )
                            .with(json!({"key": key, "detail": error.to_string()}))
                        })?;
                    if !(1..=WORKER_CANDIDATE_LIMIT).contains(&workers.len()) {
                        return Err(refuse(
                            "out_of_range",
                            format!("Keep 1 to {WORKER_CANDIDATE_LIMIT} worker candidates"),
                        )
                        .with(json!({"key": key, "min": 1, "max": WORKER_CANDIDATE_LIMIT})));
                    }
                    for candidate in &workers {
                        if candidate.description.chars().count() > WORKER_DESCRIPTION_LIMIT {
                            return Err(refuse(
                                "out_of_range",
                                format!(
                                    "Keep a worker description to {WORKER_DESCRIPTION_LIMIT} characters"
                                ),
                            )
                            .with(json!({"key": key, "max": WORKER_DESCRIPTION_LIMIT})));
                        }
                        self.worker_agent(key, candidate.agent.as_str())?;
                        candidate.launch_arguments().map_err(|detail| {
                            refuse("config_invalid", detail).with(json!({"key": key}))
                        })?;
                    }
                    config.default_runtime = workers[0].agent;
                    config.workers = workers;
                }
                "observer_mode" => {
                    config.observer_mode = ObserverMode::parse(value).ok_or_else(|| {
                        refuse("config_invalid", "Use manual, assist or autonomous").with(json!({
                            "key": key,
                            "allowed": ObserverMode::ALL.map(ObserverMode::as_str),
                        }))
                    })?
                }
                "observer_daily_limit" => {
                    let limit = value
                        .parse::<u32>()
                        .ok()
                        .filter(|limit| OBSERVER_DAILY_RANGE.contains(limit))
                        .ok_or_else(|| {
                            refuse(
                                "out_of_range",
                                format!(
                                    "Give a number from {} to {}",
                                    OBSERVER_DAILY_RANGE.start(),
                                    OBSERVER_DAILY_RANGE.end()
                                ),
                            )
                            .with(json!({
                                "key": key,
                                "min": OBSERVER_DAILY_RANGE.start(),
                                "max": OBSERVER_DAILY_RANGE.end(),
                            }))
                        })?;
                    config.observer_daily_limit = limit;
                }
                "factory_ai" => {
                    config.factory_ai = match value.trim() {
                        "" | "default" => None,
                        provider => Some(FactoryAi {
                            provider: provider.to_owned(),
                            model: None,
                            effort: None,
                        }),
                    }
                }
                "factory_ai_model" | "factory_ai_effort" => {
                    let Some(ai) = config.factory_ai.as_mut() else {
                        return Err(refuse(
                            "factory_ai_required",
                            "Choose the Factory AI's agent first: factory_ai=<provider>",
                        ));
                    };
                    let choice =
                        Some(value.trim().to_owned()).filter(|v| !v.is_empty() && v != "default");
                    if key == "factory_ai_model" {
                        ai.model = choice;
                    } else {
                        ai.effort = choice;
                    }
                }
                "harness" => {
                    config.harness = match value.split_once(':') {
                        Some((name, instructions)) => Some(Harness {
                            name: name.trim().into(),
                            instructions: instructions.trim().into(),
                        }),
                        None if value.trim().is_empty() => None,
                        None => return Err(bad()),
                    }
                }
                "autonomy" => {
                    let (scope, enabled) = value.split_once('=').ok_or_else(bad)?;
                    let enabled = enabled == "on";
                    match config.autonomy.iter_mut().find(|s| s.id == scope) {
                        Some(existing) => existing.enabled = enabled,
                        None => config.autonomy.push(AutonomyScope {
                            id: scope.into(),
                            description: scope.into(),
                            enabled,
                        }),
                    }
                }
                "recovery" => {
                    let (action, enabled) = value.split_once('=').ok_or_else(bad)?;
                    let action = RecoveryAction::ALL
                        .into_iter()
                        .find(|a| a.as_str() == action)
                        .ok_or_else(bad)?;
                    config.recovery.retain(|a| *a != action);
                    if enabled == "on" {
                        config.recovery.push(action);
                    }
                }
                "worker_args" => {
                    // `claude=--flag --other`: the runtime's whole list.
                    let (runtime, args) = value.split_once('=').ok_or_else(bad)?;
                    let runtime = Runtime::parse(runtime).ok_or_else(bad)?;
                    let args: Vec<String> = args.split_whitespace().map(str::to_owned).collect();
                    if args.is_empty() {
                        config.worker_args.remove(runtime.as_str());
                    } else {
                        config.worker_args.insert(runtime.as_str().to_owned(), args);
                    }
                }
                "risk_paths" => {
                    config.risk_paths = value
                        .split(',')
                        .map(str::trim)
                        .filter(|p| !p.is_empty())
                        .map(str::to_owned)
                        .collect()
                }
                "prd_in_issue" => config.prd_in_issue = value == "on",
                "macos_notifications" => config.macos_notifications = value == "on",
                _ => return Err(bad()),
            }
        }
        if !set.is_empty() {
            if config.merge_mode == MergeMode::Auto && !config.verification.configured() {
                return Err(refuse(
                    "auto_needs_verification",
                    "Set a verification before auto merge",
                ));
            }
            if let Some(ai) = &config.factory_ai
                && set.iter().any(|(key, _)| key.starts_with("factory_ai"))
            {
                self.ports.judge.check_ai(ai).map_err(|detail| {
                    refuse("factory_ai_unavailable", detail).with(json!({"provider": ai.provider}))
                })?;
            }
            if let Some(f) = self.factories.get_mut(&factory_id) {
                f.config = config.clone();
            }
            if let Some(workers) = machine_workers {
                self.machine_max_workers = workers;
                let _ = self
                    .store
                    .set_meta("machine.max_workers", &workers.to_string());
            }
            self.save_factory(&factory_id);
            self.record(&factory_id, None, "config.changed", json!({"keys": set.iter().map(|(k, _)| k).collect::<Vec<_>>(), "by": role.relayed_by()}));
        }
        Ok(json!({"config": config, "machine": {"max_workers": self.machine_max_workers}}))
    }

    /// An agent a worker candidate may name: one whose adapter declares a
    /// start and that this machine has (B28).
    fn worker_agent(&mut self, key: &str, value: &str) -> Result<Runtime, Refusal> {
        let allowed: Vec<&str> = Runtime::all().map(Runtime::as_str).collect();
        let agent = Runtime::parse(value).ok_or_else(|| {
            refuse("agent_not_startable", "Choose an agent Factory can start")
                .with(json!({"key": key, "allowed": allowed}))
        })?;
        if !self.ports.source.installed(agent) {
            return Err(refuse(
                "agent_not_installed",
                format!("{} is not installed on this machine", agent.label()),
            )
            .with(json!({"key": key, "agent": agent.as_str()})));
        }
        Ok(agent)
    }

    fn status(&self, project: Option<&str>) -> Reply {
        let mut summary = self.summary();
        if let Some(project) = project {
            let id = self.factory_id(Some(project))?;
            summary.factories.retain(|f| f.id == id);
        }
        let mut value = serde_json::to_value(&summary).unwrap_or_default();
        value["store_failures"] = json!(self.store_failures.borrow().total);
        Ok(value)
    }

    /// A Task's kept judgment and letter bodies, oldest first (D-58).
    pub fn records(&self, factory: &str, id: &str, limit: usize) -> Vec<Record> {
        self.store.records(factory, id, limit).unwrap_or_default()
    }

    pub fn show(&self, factory: &str, id: &str) -> Option<summary::TaskDetail> {
        let factory = self.factories.get(factory)?;
        let tasks = self.tasks.get(&factory.id)?;
        let task = tasks.get(id)?;
        let verifier = &self.ports.verifier;
        Some(summary::detail(
            factory,
            task,
            tasks,
            Self::allowed_actions(task),
            self.ports.clock.now(),
            &mut |log| verifier.log_tail(log),
        ))
    }

    // ------------------------------------------------------------------ letters

    /// Mail to a Factory (D-14): the CLI's own reports carry a typed body; a
    /// harness that only speaks the letter protocol is read as a question or
    /// a report (B25). Each letter is applied once.
    pub fn letter(&mut self, letter: Inbound) -> Value {
        if self.processed.contains(&letter.id) {
            return json!({"ok": true, "message": "already applied", "duplicate": true});
        }
        let Some((factory, task)) = self.role_for(Some(&letter.sender_pane), None) else {
            self.remember_letter(&letter.id);
            self.record(
                &letter.factory,
                None,
                "letter.unbound",
                json!({"letter": letter.id, "kind": letter.kind}),
            );
            return Refusal::new(
                "sender_not_a_worker",
                "Only a Factory worker reports to a Factory",
            )
            .to_json();
        };
        if factory != letter.factory {
            self.remember_letter(&letter.id);
            return Refusal::new(
                "sender_not_a_worker",
                "This worker belongs to another Factory",
            )
            .to_json();
        }
        self.keep(&factory, Some(&task), "letter.in", &letter.id, &letter.body);
        let role = Role::Worker {
            factory: factory.clone(),
            task: task.clone(),
        };
        let command = match serde_json::from_str::<Value>(&letter.body)
            .ok()
            .and_then(|v| v.get("factory").cloned())
        {
            Some(value) => match serde_json::from_value::<Command>(value) {
                Ok(command) => with_letter(command, &letter.id),
                Err(_) => {
                    self.remember_letter(&letter.id);
                    return Refusal::new(
                        "letter_invalid",
                        "Use hide factory ask|block|propose|done",
                    )
                    .to_json();
                }
            },
            None => plain_letter(&letter),
        };
        let answer = match command {
            Some(command) => self.command(&role, command),
            None if letter.kind == "watch" => {
                // The watch's inactivity warning about a worker: stalled (B24).
                self.stall(&factory, &task);
                json!({"ok": true, "message": "stalled"})
            }
            None => {
                Refusal::new("letter_invalid", "Use hide factory ask|block|propose|done").to_json()
            }
        };
        self.remember_letter(&letter.id);
        answer
    }

    fn remember_letter(&mut self, id: &str) {
        if self.processed.insert(id.to_owned()) {
            self.processed_order.push(id.to_owned());
            while self.processed_order.len() > PROCESSED_LETTERS {
                let old = self.processed_order.remove(0);
                self.processed.remove(&old);
            }
            if let Err(error) = self.store.set_meta(
                "processed_letters",
                &serde_json::to_string(&self.processed_order).unwrap_or_default(),
            ) {
                self.store_failed("-", None, "letters", &error.0);
            }
        }
    }

    pub fn letter_seen(&self, id: &str) -> bool {
        self.processed.contains(id)
    }

    fn stall(&mut self, factory: &str, id: &str) {
        if self
            .task(factory, id)
            .is_some_and(|t| t.state == TaskState::Running)
        {
            self.put_to_sleep(factory, id);
            self.with_task(factory, id, |task| task.stop = Some(StopReason::Stalled));
            self.set_state(factory, id, TaskState::Stopped);
            self.with_task(factory, id, |task| task.stop = Some(StopReason::Stalled));
        }
    }

    // --------------------------------------------------------------------- tick

    /// Advances everything time or the outside world moves. Each step is
    /// bounded and makes at most one external call per Task.
    pub fn tick(&mut self) {
        self.apply_judgments();
        self.poll_verifications();
        self.expire_questions();
        self.check_workers();
        self.read_outside();
        self.check_main();
        let factories: Vec<String> = self.factories.keys().cloned().collect();
        for factory in &factories {
            self.drive_revert(factory);
            self.unblock(factory);
            let publishing: Vec<String> = self
                .tasks_of(factory)
                .filter(|t| t.state == TaskState::Verifying && t.writes.contains(PUBLISH_PENDING))
                .map(|t| t.id.clone())
                .collect();
            for id in publishing {
                self.publish(factory, &id);
            }
            let verifying: Vec<String> = self
                .tasks_of(factory)
                .filter(|t| t.state == TaskState::Verifying)
                .map(|t| t.id.clone())
                .collect();
            for id in verifying {
                self.advance_merge(factory, &id);
            }
            let landed: Vec<String> = self
                .tasks_of(factory)
                .filter(|t| t.state == TaskState::Landed)
                .map(|t| t.id.clone())
                .collect();
            for id in landed {
                self.check_landed(factory, &id);
            }
            let drafting: Vec<String> = self
                .tasks_of(factory)
                .filter(|t| t.state == TaskState::Drafting)
                .map(|t| t.id.clone())
                .collect();
            for id in drafting {
                self.maybe_ready(factory, &id);
            }
            self.watch_due(factory, false);
        }
        self.start_tasks();
        self.environment_recovery();
        self.retention();
    }

    /// A Task blocked only on a predecessor (a prerequisite it proposed, a
    /// dependency added while it ran) waits for a slot again once every
    /// predecessor merged and no blocking question is open (B27, D-16).
    fn unblock(&mut self, factory: &str) {
        let Some(tasks) = self.tasks.get(factory) else {
            return;
        };
        let ready: Vec<String> = tasks
            .values()
            .filter(|t| t.state == TaskState::Blocked)
            .filter(|t| {
                !t.open_questions()
                    .any(|q| matches!(q.kind, QuestionKind::Blocking))
            })
            .filter(|t| dag::waiting_for(t, tasks).is_empty())
            .map(|t| t.id.clone())
            .collect();
        for id in ready {
            self.set_state(factory, &id, TaskState::Waiting);
        }
    }

    fn apply_judgments(&mut self) {
        for answer in self.ports.judge.finished() {
            let Some((factory, task, purpose)) = self.judgments.remove(&answer.id) else {
                continue;
            };
            let output = match &answer.outcome {
                JudgmentOutcome::Answered { value } => value.to_string(),
                JudgmentOutcome::Failed { reason } => json!({"failed": reason}).to_string(),
            };
            self.keep(
                &factory,
                task.as_deref(),
                "judgment.output",
                &answer.id,
                &output,
            );
            if matches!(
                purpose,
                Purpose::Classify { .. } | Purpose::Diagnose | Purpose::RiskMerge
            ) && let JudgmentOutcome::Failed { reason } = &answer.outcome
            {
                // A call the provider never received is not counted (D-34).
                self.observer_not_sent(&factory, reason);
            }
            // A judgment that answers after its Task was cancelled, taken
            // outside or finished asks nothing of a person.
            if let Some(id) = task.as_deref()
                && self.task(&factory, id).is_none_or(|t| {
                    matches!(
                        t.state,
                        TaskState::Cancelled | TaskState::Outside | TaskState::Done
                    )
                })
            {
                if matches!(purpose, Purpose::Drift | Purpose::Check)
                    && let Some(count) = self
                        .checks_running
                        .get_mut(&(factory.clone(), id.to_owned()))
                {
                    *count = count.saturating_sub(1);
                }
                self.record(
                    &factory,
                    Some(id),
                    "judgment.dropped",
                    json!({"purpose": format!("{purpose:?}")}),
                );
                continue;
            }
            match (&answer.outcome, purpose, task) {
                (JudgmentOutcome::Answered { value }, Purpose::Intake, Some(task)) => {
                    self.apply_intake(&factory, &task, value)
                }
                (JudgmentOutcome::Failed { reason }, Purpose::Intake, Some(task)) => {
                    self.review_failed(&factory, &task, reason)
                }
                (outcome, Purpose::Drift | Purpose::Check, Some(task)) => {
                    let key = (factory.clone(), task.clone());
                    if let Some(count) = self.checks_running.get_mut(&key) {
                        *count = count.saturating_sub(1);
                    }
                    let drafting = self
                        .task(&factory, &task)
                        .is_some_and(|t| t.state == TaskState::Drafting);
                    match outcome {
                        JudgmentOutcome::Answered { value } if drafting => {
                            // An intake check only adds questions to the draft.
                            if let Ok(finding) = judgment::parse_finding(value) {
                                for question in finding.questions {
                                    self.add_question(
                                        &factory,
                                        &task,
                                        QuestionOrigin::Check,
                                        QuestionKind::Intake,
                                        &question.text,
                                        &question.suggestion,
                                        question.default_action,
                                        None,
                                        Vec::new(),
                                        None,
                                    );
                                }
                            }
                        }
                        JudgmentOutcome::Answered { value } => {
                            self.apply_finding(&factory, &task, value, true)
                        }
                        JudgmentOutcome::Failed { .. } if drafting => {}
                        JudgmentOutcome::Failed { reason } => {
                            self.check_failed(&factory, &task, reason)
                        }
                    }
                    self.advance_merge(&factory, &task);
                }
                (JudgmentOutcome::Answered { value }, Purpose::Periodic, Some(task)) => {
                    match judgment::parse_finding(value) {
                        Ok(_) => self.apply_finding(&factory, &task, value, false),
                        Err(_) => self.record(
                            &factory,
                            Some(&task),
                            "judgment.failed",
                            json!({"purpose": "Periodic", "reason": "unreadable"}),
                        ),
                    }
                }
                (JudgmentOutcome::Answered { value }, Purpose::Watch { read_at }, _) => {
                    self.apply_watch(&factory, value, read_at)
                }
                (JudgmentOutcome::Answered { value }, Purpose::Env, _) => {
                    self.apply_diagnosis(&factory, value)
                }
                (outcome, Purpose::Classify { question }, Some(task)) => {
                    self.apply_classification(&factory, &task, &question, outcome)
                }
                (outcome, Purpose::Diagnose, Some(task)) => {
                    self.apply_worker_diagnosis(&factory, &task, outcome)
                }
                (outcome, Purpose::RiskMerge, Some(task)) => {
                    self.apply_risk_merge(&factory, &task, outcome)
                }
                (JudgmentOutcome::Failed { reason }, purpose, task) => {
                    // A watch or diagnosis that fails changes no Task (B69).
                    self.record(
                        &factory,
                        task.as_deref(),
                        "judgment.failed",
                        json!({"purpose": format!("{purpose:?}"), "reason": reason}),
                    );
                }
                _ => {}
            }
        }
    }

    fn expire_questions(&mut self) {
        let now = self.now();
        let mut expired = Vec::new();
        for task in self.all_tasks() {
            for question in task.open_questions() {
                if matches!(
                    question.kind,
                    QuestionKind::Default | QuestionKind::ScopeChange { .. }
                ) && question.deadline.is_some_and(|d| d <= now)
                {
                    expired.push((
                        task.factory.clone(),
                        task.id.clone(),
                        question.id.clone(),
                        question.default_action.clone(),
                    ));
                }
            }
        }
        for (factory, id, question, default) in expired {
            let text = default.unwrap_or_default();
            self.with_task(&factory, &id, |task| {
                if let Some(q) = task.questions.iter_mut().find(|q| q.id == question) {
                    q.answer = Some(Answer {
                        text: text.clone(),
                        chose: Some(text.clone()),
                        relayed_by: "deadline".into(),
                        at: now,
                    });
                }
            });
            self.record(
                &factory,
                Some(&id),
                "question.deadline",
                json!({"question": question}),
            );
            self.advance_merge(&factory, &id);
        }
    }

    fn check_workers(&mut self) {
        let now = self.now();
        let running: Vec<(String, String, WorkerRef)> = self
            .all_tasks()
            .filter(|t| t.state == TaskState::Running)
            .filter(|t| self.factories.get(&t.factory).is_some_and(|f| !f.paused))
            .filter_map(|t| {
                t.worker
                    .clone()
                    .map(|w| (t.factory.clone(), t.id.clone(), w))
            })
            .collect();
        for (factory, id, worker) in running {
            let key = (factory.clone(), id.clone());
            // An automatic restart on its way is asked again at its time.
            if let Some((_, next)) = self.starting.get(&key).copied() {
                if next <= now {
                    self.start(&factory, &id);
                }
                continue;
            }
            let no_report = self
                .factories
                .get(&factory)
                .map_or(2 * MINUTE_MS, |f| f.config.no_report_ms);
            match self.ports.workers.status(&worker) {
                WorkerStatus::Resting { since: rest } => {
                    self.check_rest(&factory, &id, &worker, rest, no_report, now)
                }
                WorkerStatus::Gone => self.worker_gone(&factory, &id),
                WorkerStatus::Working | WorkerStatus::Blocked => self.worker_working(&factory, &id),
                // A worker whose activity cannot be read is never resting (B43).
                WorkerStatus::Unknown => {}
            }
        }
    }

    fn start_tasks(&mut self) {
        let now = self.now();
        // A Task cancelled or paused while its worker was starting frees
        // the slot.
        let starting: Vec<_> = self.starting.keys().cloned().collect();
        for (factory, id) in starting {
            if self.task(&factory, &id).is_none_or(|t| {
                !matches!(
                    t.state,
                    TaskState::Waiting | TaskState::Relanding | TaskState::Running
                )
            }) {
                // A start the adapter took over is never replayed by its
                // intent: a later start is a new attempt. A spawn whose agent
                // had not shown yet is asked again as the same attempt, so a
                // revive gets the pane and worktree it already made.
                if self.ports.workers.abandon_start(&factory, &id) {
                    self.with_task(&factory, &id, |t| t.spawn_refusals += 1);
                }
                self.starting.remove(&(factory, id));
            }
        }
        if self.halt_until.is_some_and(|until| until > now) {
            return;
        }
        let mut candidates: Vec<Task> = self
            .all_tasks()
            // A paused Factory starts nothing (D-48).
            .filter(|t| {
                self.factories
                    .get(&t.factory)
                    .is_some_and(|f| !f.closed && !f.paused)
            })
            .filter(|t| matches!(t.state, TaskState::Waiting | TaskState::Relanding))
            .filter(|t| {
                let tasks = &self.tasks[&t.factory];
                dag::waiting_for(t, tasks).is_empty()
            })
            .cloned()
            .collect();
        if candidates.is_empty() {
            return;
        }
        // Relanding goes first; then priority, then age (D-29).
        candidates.sort_by(|a, b| {
            (b.state == TaskState::Relanding)
                .cmp(&(a.state == TaskState::Relanding))
                .then_with(|| dag::slot_order(a, b))
        });
        // Rule 1: hold starts below the disk floor or at critical memory
        // pressure; re-checked when a worker ends or a minute passes (B57).
        // Without a hold the machine is read before a start, and only when
        // one can happen: a backlog waiting on full slots does not read the
        // disk and fork `sysctl` on every tick.
        let usable = |runtime: Runtime| {
            self.runtime_blocked
                .get(&runtime)
                .is_none_or(|until| *until <= now)
        };
        let can_start = (self.machine_max_workers > self.running_count()
            && Runtime::all().any(usable))
            || candidates.iter().any(|t| {
                t.state == TaskState::Relanding
                    || self
                        .starting
                        .get(&(t.factory.clone(), t.id.clone()))
                        .is_some_and(|(_, next)| *next <= now)
            });
        let due = self
            .hold_checked_at
            .is_none_or(|at| now.saturating_sub(at) >= ENV_RECHECK_MS)
            || (self.hold_reason.is_none() && can_start);
        if due {
            self.hold_checked_at = Some(now);
            self.hold_reason = None;
            let project = self
                .factories
                .get(&candidates[0].factory)
                .map(|f| f.project.clone())
                .unwrap_or_default();
            let floor = self
                .factories
                .get(&candidates[0].factory)
                .map_or(20 << 30, |f| f.config.disk_floor_bytes);
            if self
                .ports
                .environment
                .disk_free(&project)
                .is_some_and(|free| free < floor)
            {
                self.hold_reason = Some(EnvHold::DiskFloor);
            } else if self.ports.environment.memory_pressure() == MemoryPressure::Critical {
                self.hold_reason = Some(EnvHold::MemoryCritical);
            }
        }
        if let Some(reason) = self.hold_reason {
            self.env_problem_since.get_or_insert(now);
            for task in &candidates {
                if task.held_code != Some(reason) {
                    self.with_task(&task.factory, &task.id, |t| {
                        t.held = Some(reason.label().to_owned());
                        t.held_code = Some(reason);
                    });
                }
            }
            return;
        }
        for task in &candidates {
            if task.held.is_some() || task.held_code.is_some() {
                self.with_task(&task.factory, &task.id, |t| {
                    t.held = None;
                    t.held_code = None;
                });
            }
        }
        let mut free = self
            .machine_max_workers
            .saturating_sub(self.running_count());
        for task in candidates {
            let key = (task.factory.clone(), task.id.clone());
            if let Some((_, next)) = self.starting.get(&key) {
                // Its slot is already counted; ask again when due.
                if *next <= now {
                    self.start(&task.factory, &task.id);
                }
                continue;
            }
            // A relanding Task already holds its slot.
            let relanding = task.state == TaskState::Relanding;
            if free == 0 && !relanding {
                continue;
            }
            if self.start(&task.factory, &task.id) && !relanding {
                free -= 1;
            }
        }
    }

    /// A runtime whose usage the machine reports used up is not started
    /// until its reset (B58).
    fn read_usage_limits(&mut self) {
        // Every agent whose adapter declares a usage reading (D-52).
        for runtime in Runtime::all().filter(|r| r.adapter().usage.is_some()) {
            if let Some(until) = self.ports.workers.usage_limited(runtime) {
                let entry = self.runtime_blocked.entry(runtime).or_insert(until);
                *entry = (*entry).max(until);
            }
        }
    }

    fn start(&mut self, factory_id: &str, id: &str) -> bool {
        let Some(factory) = self.factories.get(factory_id).cloned() else {
            return false;
        };
        let Some(task) = self.task(factory_id, id).cloned() else {
            return false;
        };
        let now = self.now();
        self.read_usage_limits();
        self.runtime_blocked.retain(|_, until| *until > now);
        let (mut candidate, index, pinned) = task.candidate(&factory);
        if let Some(worker) = &task.worker
            && task.launched.is_none()
        {
            // A worker started before candidates resumes as it started.
            candidate = WorkerCandidate {
                model: worker.model.clone(),
                effort: worker.effort.clone(),
                ..WorkerCandidate::bare(worker.runtime)
            };
        }
        if self.runtime_blocked.contains_key(&candidate.agent) {
            // A new start moves to the next candidate whose usage is not
            // used up; a pinned one, or a worker resuming, waits (D-42).
            if pinned || task.worker.is_some() {
                return false;
            }
            let candidates = factory.config.candidates();
            let from = index.map_or(0, |i| i + 1);
            let Some(next) = (0..candidates.len())
                .map(|offset| &candidates[(from + offset) % candidates.len()])
                .find(|c| !self.runtime_blocked.contains_key(&c.agent))
            else {
                return false;
            };
            candidate = next.clone();
        }
        let runtime = candidate.agent;
        let mut args = factory
            .config
            .worker_args
            .get(runtime.as_str())
            .cloned()
            .unwrap_or_default();
        match candidate.launch_arguments() {
            Ok(extra) => args.extend(extra),
            Err(detail) => {
                // The candidate names a model or effort its agent does not take.
                self.set_state(factory_id, id, TaskState::Stopped);
                let detail = judgment::cut(&detail, 300);
                self.with_task(factory_id, id, |t| {
                    t.stop = Some(StopReason::WorkerStart);
                    t.stop_detail = Some(detail);
                });
                return false;
            }
        }
        let relanding = task.state == TaskState::Relanding;
        // A Task with a worker resumes it (wake after answer, pause, retry,
        // relanding); otherwise a new worker starts (B22, B54).
        if let Some(worker) = task.worker.clone() {
            let pending: Vec<String> = task
                .flags
                .iter()
                .filter_map(|f| f.strip_prefix("pending reply: ").map(str::to_owned))
                .collect();
            let gone = matches!(self.ports.workers.status(&worker), WorkerStatus::Gone);
            let body = if relanding {
                "Factory: 이 Task의 머지가 main 검증 실패로 revert되었습니다. 최신 main 위로 다시 올리고 다시 hide factory done 하세요.".to_owned()
            } else if gone {
                String::new()
            } else if pending.is_empty() {
                "Factory: 이어서 진행하세요.".to_owned()
            } else {
                pending.join("\n")
            };
            let restarted = if gone {
                // Retry: the same worktree and session, a fresh process (B54).
                let request = WorkerSpawn {
                    factory: factory_id.to_owned(),
                    task: id.to_owned(),
                    name: worker.name.clone(),
                    runtime,
                    project: factory.project.clone(),
                    branch: worker.branch.clone(),
                    prompt: worker_prompt(&task, &factory, true, &self.hide_program),
                    args: args.clone(),
                    model: candidate.model.clone(),
                    effort: candidate.effort.clone(),
                    resume: Some(worker.clone()),
                    attempt: task.spawn_refusals,
                };
                self.ports.workers.spawn(&request).map(Some)
            } else {
                self.ports.workers.wake(&worker, &body).map(|()| None)
            };
            let key = (factory_id.to_owned(), id.to_owned());
            match restarted {
                Ok(new_worker) => {
                    self.starting.remove(&key);
                    let restart = new_worker.is_some();
                    self.with_task(factory_id, id, |t| {
                        if let Some(w) = new_worker {
                            t.worker = Some(w);
                        }
                        if let Some(w) = &mut t.worker {
                            w.asleep = false;
                        }
                        t.flags.retain(|f| !f.starts_with("pending reply: "));
                        t.last_report_at = None;
                        t.recovery = None;
                        t.woken_at = Some(now);
                    });
                    if restart {
                        self.record(
                            factory_id,
                            Some(id),
                            "worker.restarted",
                            json!({"runtime": runtime.as_str(), "auto_restarts": task.auto_restarts}),
                        );
                    }
                    self.set_state(factory_id, id, TaskState::Running);
                    true
                }
                // A restart still on its way holds the slot like a new one.
                Err(failure) if failure.starting => {
                    self.worker_starting(factory_id, id, &failure);
                    true
                }
                Err(failure) => {
                    self.starting.remove(&key);
                    self.external_failure(factory_id, Some(id), &failure);
                    false
                }
            }
        } else {
            let request = WorkerSpawn {
                factory: factory_id.to_owned(),
                task: id.to_owned(),
                name: worker_name(&factory, &task),
                runtime,
                project: factory.project.clone(),
                branch: task.branch_slug(),
                prompt: worker_prompt(&task, &factory, false, &self.hide_program),
                args,
                model: candidate.model.clone(),
                effort: candidate.effort.clone(),
                resume: None,
                attempt: task.spawn_refusals,
            };
            let key = (factory_id.to_owned(), id.to_owned());
            match self.ports.workers.spawn(&request) {
                Ok(worker) => {
                    self.starting.remove(&key);
                    let launched = candidate.clone();
                    self.with_task(factory_id, id, |t| {
                        t.worker = Some(worker);
                        t.last_report_at = None;
                        t.launched = Some(launched);
                        t.recovery = None;
                        t.woken_at = Some(now);
                    });
                    self.record(
                        factory_id,
                        Some(id),
                        "worker.started",
                        json!({
                            "runtime": runtime.as_str(),
                            "model": candidate.model,
                            "effort": candidate.effort,
                        }),
                    );
                    self.set_state(factory_id, id, TaskState::Running);
                    true
                }
                Err(failure) if failure.starting => {
                    self.worker_starting(factory_id, id, &failure);
                    true
                }
                Err(failure) => {
                    self.starting.remove(&key);
                    self.external_failure(factory_id, Some(id), &failure);
                    if failure.signal.is_none() {
                        // A refused start does not change by asking again;
                        // a person fixes the cause and retries (B54).
                        self.set_state(factory_id, id, TaskState::Stopped);
                        let detail = judgment::cut(&failure.detail, 300);
                        self.with_task(factory_id, id, |t| {
                            t.stop = Some(StopReason::WorkerStart);
                            t.stop_detail = Some(detail);
                            t.spawn_refusals += 1;
                        });
                    }
                    false
                }
            }
        }
    }

    /// The worker's pane runs but its agent has shown no session: ask the
    /// same spawn again later, and after a while tell the person to look
    /// at the pane, where a first-run prompt may be waiting.
    fn worker_starting(&mut self, factory: &str, id: &str, failure: &Failure) {
        let now = self.now();
        let key = (factory.to_owned(), id.to_owned());
        let first = match self.starting.get(&key) {
            Some((first, _)) => *first,
            None => {
                self.record(
                    factory,
                    Some(id),
                    "worker.starting",
                    json!({"detail": failure.detail}),
                );
                now
            }
        };
        let again = failure.again_in_ms.unwrap_or(START_RETRY_MS);
        self.starting.insert(key, (first, now + again));
        if now.saturating_sub(first) >= START_NOTICE_MS {
            let name = self
                .task(factory, id)
                .map(|t| worker_name(&self.factories[factory], t))
                .unwrap_or_default();
            self.once_notice(
                factory,
                id,
                &format!("worker {name}가 시작되지 않았습니다. 그 pane에서 신뢰·로그인 같은 확인 화면이 기다리는지 보세요."),
            );
        }
    }

    // ------------------------------------------------------------ outside work

    fn read_outside(&mut self) {
        let now = self.now();
        let due: Vec<String> = self
            .factories
            .values()
            .filter(|f| !f.closed)
            .filter(|f| {
                f.outside_read_at
                    .is_none_or(|at| now.saturating_sub(at) >= f.config.outside_read_ms)
            })
            .filter(|f| {
                self.github_backoff
                    .get(&f.id)
                    .is_none_or(|(_, until)| *until <= now)
            })
            .map(|f| f.id.clone())
            .collect();
        for factory_id in due {
            let Some(factory) = self.factories.get(&factory_id).cloned() else {
                continue;
            };
            let tasks: Vec<Task> = self.tasks_of(&factory_id).cloned().collect();
            let refs: Vec<&Task> = tasks.iter().collect();
            match self.ports.source.observe(&factory, &refs) {
                Ok(events) => {
                    self.github_backoff.remove(&factory_id);
                    if let Some(f) = self.factories.get_mut(&factory_id) {
                        f.outside_read_at = Some(now);
                        f.outside_read_failures = 0;
                    }
                    self.save_factory(&factory_id);
                    for event in events {
                        self.apply_outside(&factory_id, event);
                    }
                }
                Err(failure) => {
                    if let Some(f) = self.factories.get_mut(&factory_id) {
                        f.outside_read_failures += 1;
                        f.outside_read_at = Some(now);
                    }
                    self.save_factory(&factory_id);
                    self.external_failure(&factory_id, None, &failure);
                }
            }
        }
    }

    fn task_for_issue(&self, factory: &str, issue: &IssueRef) -> Option<Task> {
        self.tasks_of(factory)
            .find(|t| t.issue.as_ref() == Some(issue))
            .cloned()
    }

    /// The person wins outside; the Factory follows (D-27, B49-B52, B15).
    fn apply_outside(&mut self, factory_id: &str, event: OutsideEvent) {
        let factory = self.factories.get(factory_id).cloned();
        match event {
            OutsideEvent::ClosingPr {
                issue,
                pr,
                url,
                merged,
            } => {
                let Some(task) = self.task_for_issue(factory_id, &issue) else {
                    return;
                };
                if task.pr.as_ref().is_some_and(|p| p.number == pr) {
                    return;
                }
                match task.state {
                    TaskState::Done | TaskState::Landed | TaskState::Cancelled => {}
                    _ if merged => {
                        self.record(
                            factory_id,
                            Some(&task.id),
                            "outside.merged",
                            json!({"pr": pr}),
                        );
                        self.cancel_verification(factory_id, &task.id);
                        self.follow_outside(factory_id, &task);
                        self.set_state(factory_id, &task.id, TaskState::Done);
                    }
                    TaskState::Outside => {}
                    TaskState::Running
                    | TaskState::Verifying
                    | TaskState::Blocked
                    | TaskState::Paused
                    | TaskState::MergeWaiting
                    | TaskState::Stopped => {
                        self.cancel_verification(factory_id, &task.id);
                        self.follow_outside(factory_id, &task);
                        self.set_state(factory_id, &task.id, TaskState::Outside);
                        self.notice(factory_id, &task.id, &format!("밖의 PR이 이 Task의 issue를 닫습니다: {url}. worker를 멈췄고 worktree는 7일 남습니다. 되살리려면 hide factory revive {}", task.display_id()));
                    }
                    _ => {
                        self.follow_outside(factory_id, &task);
                        self.set_state(factory_id, &task.id, TaskState::Outside);
                        self.record(factory_id, Some(&task.id), "outside.pr", json!({"pr": pr}));
                    }
                }
            }
            OutsideEvent::IssueClosed { issue } | OutsideEvent::LabelRemoved { issue } => {
                let Some(task) = self.task_for_issue(factory_id, &issue) else {
                    return;
                };
                if matches!(
                    task.state,
                    TaskState::Done | TaskState::Landed | TaskState::Cancelled
                ) {
                    return;
                }
                if task.state == TaskState::Outside {
                    // Its outside PR closed the issue: follow it to done; the
                    // stopped worker's worktree keeps its period (B50).
                    self.set_state(factory_id, &task.id, TaskState::Done);
                    return;
                }
                self.cancel(factory_id, &task.id);
                self.notice(factory_id, &task.id, "issue가 PR 없이 닫혔거나 factory 라벨이 빠져 Task를 취소했습니다. 7일 동안 되살릴 수 있습니다.");
            }
            OutsideEvent::IssueReopened { issue } => {
                if let Some(task) = self.task_for_issue(factory_id, &issue)
                    && task.state == TaskState::Done
                {
                    self.notice(
                        factory_id,
                        &task.id,
                        "완료한 Task의 issue가 다시 열렸습니다. 자동으로 다시 넣지 않았습니다.",
                    );
                }
            }
            OutsideEvent::BodyEdited {
                issue,
                body_hash,
                body,
            } => {
                let Some(task) = self.task_for_issue(factory_id, &issue) else {
                    return;
                };
                if task.source_body_hash.as_deref() == Some(body_hash.as_str()) {
                    return;
                }
                let first = task.source_body_hash.is_none();
                self.with_task(factory_id, &task.id, |t| {
                    t.source_body_hash = Some(body_hash.clone());
                    t.card.summary = Some(goal_summary(&body, &t.card.title));
                });
                if first {
                    return;
                }
                match task.state {
                    TaskState::Drafting | TaskState::Waiting => {
                        // Before start: back to drafting and reviewed again (B52).
                        self.set_state(factory_id, &task.id, TaskState::Drafting);
                        let now = self.now();
                        self.request_review(factory_id, &task.id, now);
                    }
                    TaskState::Running
                    | TaskState::Blocked
                    | TaskState::Paused
                    | TaskState::Verifying => {
                        let deadline = factory
                            .as_ref()
                            .map_or(24 * HOUR_MS, |f| f.config.question_deadline_ms);
                        let now = self.now();
                        self.add_question(
                            factory_id,
                            &task.id,
                            QuestionOrigin::Engine,
                            QuestionKind::ScopeChange { change: None },
                            "사람이 issue 본문을 고쳤습니다. 새 범위를 승인할까요?",
                            "approve",
                            Some("지금 범위로 진행".into()),
                            Some(now + deadline),
                            vec!["approve".into(), "reject".into()],
                            None,
                        );
                    }
                    _ => {}
                }
            }
            OutsideEvent::Labeled { issue, title, body } => {
                if self.task_for_issue(factory_id, &issue).is_some()
                    || factory.as_ref().is_some_and(|f| f.closed)
                {
                    return;
                }
                // The label path: the review drafts the card from the body and
                // a person confirms it (B15, D-48); the body is not touched.
                let card = Card {
                    title: judgment::cut(&title, TITLE_LIMIT),
                    goal: judgment::cut(&body, TEXT_LIMIT),
                    criteria: extract_criteria(&body),
                    ..Card::default()
                };
                let id = self.new_task(factory_id, card, None, None);
                self.with_task(factory_id, &id, |t| {
                    t.issue = Some(issue.clone());
                    t.label_path = true;
                    t.writes.insert("label".into());
                    t.source_body_hash = Some(crate::store::body_hash(&body));
                });
                self.add_question(
                    factory_id,
                    &id,
                    QuestionOrigin::Engine,
                    QuestionKind::ConfirmCard,
                    "라벨로 들어온 Task입니다. 카드 초안을 확인해 주세요.",
                    "confirm",
                    None,
                    None,
                    vec!["confirm".into(), "cancel".into()],
                    None,
                );
                let now = self.now();
                self.request_review(factory_id, &id, now);
            }
            OutsideEvent::OutsidePush { sha } => {
                self.record(factory_id, None, "outside.push", json!({"sha": sha}));
            }
        }
    }

    /// Reads main's head and its verification for pushes the Factory did not
    /// make (B47) and recovery back to green (D-47).
    fn check_main(&mut self) {
        let factories: Vec<Factory> = self
            .factories
            .values()
            .filter(|f| !f.closed)
            .cloned()
            .collect();
        for factory in factories {
            let now = self.now();
            let paced = self
                .main_checked_at
                .get(&factory.id)
                .is_some_and(|at| now.saturating_sub(*at) < MAIN_CHECK_EVERY_MS);
            let due = !self.main_seen.contains_key(&factory.id)
                || factory
                    .outside_read_at
                    .is_some_and(|at| now.saturating_sub(at) < 1000)
                || ((factory.main.broken || self.main_pending.contains(&factory.id)) && !paced);
            if !due {
                continue;
            }
            self.main_checked_at.insert(factory.id.clone(), now);
            let Ok(head) = self.ports.merge.main_head(&factory) else {
                continue;
            };
            if !factory.main.broken && !self.main_seen.contains_key(&factory.id) {
                // The head found at start is the baseline, not a push (B47).
                self.main_seen.insert(factory.id.clone(), head);
                continue;
            }
            let known = factory
                .main
                .merges_since_green
                .iter()
                .any(|m| m.sha == head);
            if self.main_seen.get(&factory.id) == Some(&head) && !factory.main.broken {
                continue;
            }
            match self.ports.merge.main_check(&factory, &head) {
                Ok(MainCheck::Green) => {
                    self.main_pending.remove(&factory.id);
                    self.main_seen.insert(factory.id.clone(), head.clone());
                    if factory.main.broken && !self.reverts.contains_key(&factory.id) {
                        // Green again: auto merge resumes (D-47).
                        if let Some(f) = self.factories.get_mut(&factory.id) {
                            f.main.broken = false;
                            f.main.needs_person = false;
                            f.main.reason = None;
                            f.main.last_green = Some(head.clone());
                        }
                        self.save_factory(&factory.id);
                        self.record(&factory.id, None, "main.green", json!({"sha": head}));
                    }
                }
                // A merge that may have landed without its commit named yet
                // is not an outside push: wait until the commit is known.
                Ok(MainCheck::Red { .. }) if !known && self.merge_unnamed(&factory.id) => {
                    self.main_pending.insert(factory.id.clone());
                }
                Ok(MainCheck::Red { link }) if !known && !factory.main.broken => {
                    self.main_pending.remove(&factory.id);
                    self.main_seen.insert(factory.id.clone(), head.clone());
                    self.main_broken(&factory.id, &head, &link);
                }
                // Not finished: the head is not seen yet, so a red result
                // that arrives later still counts (B47).
                Ok(MainCheck::Pending) => {
                    if !factory.main.broken {
                        self.main_pending.insert(factory.id.clone());
                    }
                }
                Ok(_) => {
                    self.main_pending.remove(&factory.id);
                    if !factory.main.broken {
                        self.main_seen.insert(factory.id.clone(), head);
                    }
                }
                Err(failure) => self.external_failure(&factory.id, None, &failure),
            }
        }
    }

    /// A Task of this Factory still verifying whose merge answered without
    /// naming its commit.
    fn merge_unnamed(&self, factory_id: &str) -> bool {
        self.merge_retry
            .iter()
            .filter(|((factory, _), (_, since))| {
                factory == factory_id
                    && since.is_some_and(|since| {
                        self.now().saturating_sub(since) < MERGE_UNNAMED_LIMIT_MS
                    })
            })
            .any(|((factory, id), _)| {
                self.task(factory, id)
                    .is_some_and(|t| t.state == TaskState::Verifying)
            })
    }

    // ------------------------------------------------------------- environment

    /// A failure the Task did not cause is not counted (D-31 rule 2, B58).
    fn verification_environment(
        &mut self,
        factory: &str,
        id: &str,
        failure: &Failure,
        stage: &str,
    ) {
        let now = self.now();
        self.with_task(factory, id, |task| task.environment_failures += 1);
        self.env_failures.push(FailureNote {
            at: now,
            factory: factory.to_owned(),
            task: id.to_owned(),
            stage: stage.to_owned(),
            kind: FailureKind::Environment,
        });
        let repeated = self.task(factory, id).map_or(0, |t| t.environment_failures);
        // Another Task's environment failure in the window says the cause is
        // shared; a Task failure elsewhere says nothing about it.
        let others_fine = !self.env_failures.iter().any(|note| {
            note.kind == FailureKind::Environment
                && now.saturating_sub(note.at) <= CASCADE_WINDOW_MS
                && note.factory == factory
                && note.task != id
        });
        if repeated >= 3 && others_fine {
            // Only this Task keeps failing this way: the Task's (B60).
            self.with_task(factory, id, |task| {
                task.stop = Some(StopReason::EnvironmentRepeated)
            });
            self.set_state(factory, id, TaskState::Stopped);
            self.with_task(factory, id, |task| {
                task.stop = Some(StopReason::EnvironmentRepeated)
            });
            self.add_question(
                factory,
                id,
                QuestionOrigin::Engine,
                QuestionKind::Action,
                &format!("같은 환경 실패가 이 Task에서만 3번 반복되었습니다 ({stage})."),
                "retry",
                None,
                None,
                vec!["retry".into(), "cancel".into()],
                None,
            );
            return;
        }
        self.external_failure(factory, Some(id), failure);
        if self
            .task(factory, id)
            .is_some_and(|t| t.state == TaskState::Verifying)
        {
            // Back to waiting; the worker sleeps until the environment clears.
            self.set_state(factory, id, TaskState::Waiting);
        }
    }

    fn cascade_note(&mut self, factory: &str, id: &str, stage: &str, now: UnixMs) {
        self.env_failures
            .retain(|note| now.saturating_sub(note.at) <= CASCADE_WINDOW_MS);
        self.env_failures.push(FailureNote {
            at: now,
            factory: factory.to_owned(),
            task: id.to_owned(),
            stage: stage.to_owned(),
            kind: FailureKind::Task,
        });
    }

    /// Three different Tasks failing the same stage within 30 minutes is the
    /// environment: their counts go back and new starts halt (B60).
    fn reclassified_by_cascade(&mut self, factory: &str, id: &str) -> bool {
        let now = self.now();
        let stage = self
            .env_failures
            .iter()
            .rev()
            .find(|note| {
                note.kind == FailureKind::Task && note.factory == factory && note.task == id
            })
            .map(|note| note.stage.clone());
        let Some(stage) = stage else { return false };
        let tasks: BTreeSet<String> = self
            .env_failures
            .iter()
            .filter(|note| {
                note.kind == FailureKind::Task
                    && now.saturating_sub(note.at) <= CASCADE_WINDOW_MS
                    && note.factory == factory
                    && note.stage == stage
            })
            .map(|note| note.task.clone())
            .collect();
        if tasks.len() < 3 {
            return false;
        }
        for task in &tasks {
            self.with_task(factory, task, |t| {
                t.failures = t.failures.saturating_sub(1);
            });
            if self.task(factory, task).is_some_and(|t| {
                matches!(
                    t.state,
                    TaskState::Running | TaskState::Verifying | TaskState::Stopped
                )
            }) {
                self.put_to_sleep(factory, task);
                self.set_state(factory, task, TaskState::Waiting);
            }
        }
        self.env_failures.retain(|note| {
            !(note.kind == FailureKind::Task && note.factory == factory && note.stage == stage)
        });
        self.halt_until = Some(now + CASCADE_WINDOW_MS);
        self.env_problem_since.get_or_insert(now);
        self.record(
            factory,
            None,
            "environment.cascade",
            json!({"stage": stage, "tasks": tasks.len()}),
        );
        true
    }

    /// Code handles each structured signal first (D-31, Q40, B58, B59).
    fn external_failure(&mut self, factory: &str, task: Option<&str>, failure: &Failure) {
        self.record(
            factory,
            task,
            "external.failed",
            json!({
                "stage": failure.stage,
                "signal": failure.signal.map(EnvSignal::as_str),
                "detail": judgment::cut(&failure.detail, 200),
            }),
        );
        let now = self.now();
        let Some(signal) = failure.signal else { return };
        self.env_problem_since.get_or_insert(now);
        let anchor = task
            .map(str::to_owned)
            .or_else(|| self.tasks_of(factory).last().map(|t| t.id.clone()));
        match signal {
            EnvSignal::GithubRateLimit | EnvSignal::GithubServer | EnvSignal::Network => {
                let (attempts, _) = self.github_backoff.get(factory).copied().unwrap_or((0, 0));
                let wait = BACKOFF_MS[(attempts as usize).min(BACKOFF_MS.len() - 1)];
                self.github_backoff
                    .insert(factory.to_owned(), (attempts + 1, now + wait));
            }
            EnvSignal::GithubAuth => {
                if let Some(anchor) = anchor {
                    self.once_notice(
                        factory,
                        &anchor,
                        "GitHub 로그인이 풀렸습니다. gh auth login 으로 다시 로그인하세요.",
                    );
                }
            }
            EnvSignal::GithubForbidden => {
                if let Some(anchor) = anchor {
                    let scope = failure
                        .missing_scope
                        .clone()
                        .unwrap_or_else(|| "repo".into());
                    self.once_notice(
                        factory,
                        &anchor,
                        &format!(
                            "GitHub 권한이 모자라 {}을 하지 않았습니다. gh auth refresh -s {scope}",
                            failure.stage
                        ),
                    );
                }
            }
            EnvSignal::DiskFull => {
                self.hold_reason = Some(EnvHold::DiskFull);
                self.hold_checked_at = Some(now);
                self.remove_finished_worktrees(None);
            }
            EnvSignal::OutOfMemory => {
                if let Some(task) = task
                    && self.task(factory, task).is_some_and(|t| {
                        matches!(t.state, TaskState::Running | TaskState::Verifying)
                    })
                {
                    self.set_state(factory, task, TaskState::Waiting);
                }
            }
            EnvSignal::HerdrSocket => {
                // Reconnect is the adapter's; the worker restarts in the same
                // worktree and session at its next start.
                if let Some(task) = task
                    && self
                        .task(factory, task)
                        .is_some_and(|t| t.state == TaskState::Running)
                {
                    self.set_state(factory, task, TaskState::Waiting);
                }
            }
            EnvSignal::UsageLimit => {
                let runtime = task
                    .and_then(|t| self.task(factory, t))
                    .and_then(|t| t.worker.as_ref().map(|w| w.runtime));
                if let Some(runtime) = runtime {
                    self.runtime_blocked
                        .insert(runtime, failure.reset_at.unwrap_or(now + HOUR_MS));
                }
                if let Some(task) = task
                    && self
                        .task(factory, task)
                        .is_some_and(|t| t.state == TaskState::Running)
                {
                    self.set_state(factory, task, TaskState::Waiting);
                }
            }
        }
    }

    fn once_notice(&mut self, factory: &str, id: &str, text: &str) {
        let exists = self
            .task(factory, id)
            .is_some_and(|t| t.open_questions().any(|q| q.text == text));
        if !exists {
            self.notice(factory, id, text);
        }
    }

    /// D-54 ①: only worktrees of finished Tasks and of cancelled Tasks past
    /// their keep period (B62), of one Factory or, below the disk floor, of
    /// every Factory.
    fn remove_finished_worktrees(&mut self, only: Option<&str>) {
        let now = self.now();
        let targets: Vec<(String, String, WorkerRef)> = self
            .all_tasks()
            .filter(|t| only.is_none_or(|factory| t.factory == factory))
            .filter(|t| !t.purged)
            .filter(|t| {
                let keep = self
                    .factories
                    .get(&t.factory)
                    .map_or(7 * DAY_MS, |f| f.config.cancel_keep_ms);
                (t.state == TaskState::Done && t.cancelled_at.is_none())
                    || (kept_for_revive(t)
                        && t.cancelled_at
                            .is_some_and(|at| now.saturating_sub(at) > keep))
            })
            .filter_map(|t| {
                t.worker
                    .clone()
                    .map(|w| (t.factory.clone(), t.id.clone(), w))
            })
            .collect();
        for (factory, id, worker) in targets {
            let removal = match self.task(&factory, &id).is_some_and(kept_for_revive) {
                true => Removal::Discarded,
                false => Removal::Finished,
            };
            match self.ports.workers.remove_worktree(&worker, removal) {
                Ok(()) => {
                    self.with_task(&factory, &id, |t| t.purged = true);
                    self.record(&factory, Some(&id), "cleanup.worktree", json!({}));
                }
                Err(failure) => self.cleanup_failed(&factory, &id, &failure),
            }
        }
    }

    fn environment_recovery(&mut self) {
        let now = self.now();
        let Some(since) = self.env_problem_since else {
            return;
        };
        let still = self.hold_reason.is_some() || self.halt_until.is_some_and(|u| u > now);
        if !still {
            self.env_problem_since = None;
            self.env_diagnosed = false;
            return;
        }
        if self.env_diagnosed || now.saturating_sub(since) < CASCADE_WINDOW_MS {
            return;
        }
        // Not cleared in 30 minutes: a judgment diagnoses (B61).
        let Some(factory) = self.factories.values().find(|f| !f.closed).cloned() else {
            return;
        };
        self.env_diagnosed = true;
        let facts = json!({
            "hold": self.hold_reason.map(EnvHold::label),
            "halted": self.halt_until.is_some_and(|u| u > now),
            "recent_failures": self.env_failures.iter().map(|note| json!({"task": note.task, "stage": note.stage, "kind": note.kind.as_str()})).collect::<Vec<_>>(),
            "disk_free": self.ports.environment.disk_free(&factory.project),
        });
        let judgment = Judgment {
            id: format!("{}:env:{now}", factory.id),
            factory: factory.id.clone(),
            task: None,
            priority: Priority::Factory,
            input: JudgmentInput::EnvDiagnosis {
                facts,
                actions: RecoveryAction::ALL.to_vec(),
            },
            ai: None,
        };
        if self.submit_judgment(judgment.clone()).is_ok() {
            self.judgments
                .insert(judgment.id, (factory.id.clone(), None, Purpose::Env));
        }
    }

    /// One action of the closed list (D-54), on this Factory's Tasks only.
    fn run_recovery(&mut self, factory: &str, action: RecoveryAction) {
        self.record(
            factory,
            None,
            "recovery.run",
            json!({"action": action.as_str()}),
        );
        match action {
            RecoveryAction::RemoveFinishedWorktrees => {
                self.remove_finished_worktrees(Some(factory));
            }
            // The start hold is the machine's (disk, memory): reading it again
            // is not one Factory's to keep.
            RecoveryAction::RetryReadsAndReconnect => {
                self.github_backoff.remove(factory);
                self.hold_reason = None;
            }
            // A Task the environment stopped starts its worker again in the
            // same worktree and session (B54).
            RecoveryAction::RestartWorker => {
                let stopped: Vec<String> = self
                    .tasks_of(factory)
                    .filter(|t| t.state == TaskState::Stopped)
                    .filter(|t| {
                        matches!(
                            t.stop,
                            Some(
                                StopReason::WorkerStart
                                    | StopReason::EnvironmentRepeated
                                    | StopReason::NoReport
                                    | StopReason::Stalled
                            )
                        )
                    })
                    .map(|t| t.id.clone())
                    .collect();
                for id in stopped {
                    self.with_task(factory, &id, |task| task.environment_failures = 0);
                    self.record(factory, Some(&id), "recovery.restart_worker", json!({}));
                    self.set_state(factory, &id, TaskState::Waiting);
                }
            }
            // A running worker waiting on input is asked to sleep and woken
            // in the same session with a note to carry on. Herdr does not put
            // an agent waiting for a person to sleep, so for such an agent the
            // note waits for its next prompt.
            RecoveryAction::SleepWakeWorker => {
                let stuck: Vec<(String, WorkerRef)> = self
                    .tasks_of(factory)
                    .filter(|t| t.state == TaskState::Running)
                    .filter_map(|t| t.worker.clone().map(|w| (t.id.clone(), w)))
                    .collect();
                for (id, worker) in stuck {
                    if self.ports.workers.status(&worker) != WorkerStatus::Blocked {
                        continue;
                    }
                    self.record(factory, Some(&id), "recovery.sleep_wake_worker", json!({}));
                    self.put_to_sleep(factory, &id);
                    self.wake(
                        factory,
                        &id,
                        "Factory: 환경 복구로 다시 깨웠습니다. 하던 일을 이어가고 끝나면 hide factory done 하세요.",
                    );
                }
            }
            // New starts of an unpinned Task leave the Factory's default
            // runtime for an hour, while the other one is not limited.
            RecoveryAction::SwitchRuntime => {
                let Some(candidates) = self.factories.get(factory).map(|f| f.config.candidates())
                else {
                    return;
                };
                let runtime = candidates[0].agent;
                let now = self.now();
                // Only when another candidate's agent can take the starts.
                if !candidates.iter().any(|c| {
                    c.agent != runtime
                        && self
                            .runtime_blocked
                            .get(&c.agent)
                            .is_none_or(|until| *until <= now)
                }) {
                    return;
                }
                let until = now + HOUR_MS;
                let entry = self.runtime_blocked.entry(runtime).or_insert(until);
                *entry = (*entry).max(until);
            }
        }
    }

    fn apply_diagnosis(&mut self, factory: &str, value: &Value) {
        let Ok(diagnosis) = judgment::parse_env(value) else {
            return;
        };
        let enabled = self
            .factories
            .get(factory)
            .map(|f| f.config.recovery.clone())
            .unwrap_or_default();
        let anchor = self.tasks_of(factory).last().map(|t| t.id.clone());
        match diagnosis.action {
            Some(action) if enabled.contains(&action) => {
                self.run_recovery(factory, action);
            }
            Some(action) => {
                if let Some(anchor) = anchor {
                    self.add_question(
                        factory,
                        &anchor,
                        QuestionOrigin::Engine,
                        QuestionKind::Proposal {
                            command: action.as_str().into(),
                            impact: diagnosis.cause.clone(),
                        },
                        &format!(
                            "환경 문제: {}. 복구 동작 {}을 실행할까요?",
                            diagnosis.cause,
                            action.as_str()
                        ),
                        "approve",
                        None,
                        None,
                        vec!["approve".into(), "dismiss".into()],
                        None,
                    );
                }
            }
            None => {
                if let (Some(anchor), Some((command, impact))) = (anchor, diagnosis.proposal) {
                    self.add_question(
                        factory,
                        &anchor,
                        QuestionOrigin::Engine,
                        QuestionKind::Proposal {
                            command: command.clone(),
                            impact: impact.clone(),
                        },
                        &format!(
                            "환경 문제: {}. 사람이 실행할 명령: {command} (영향: {impact})",
                            diagnosis.cause
                        ),
                        "run it yourself",
                        None,
                        None,
                        vec!["done".into(), "dismiss".into()],
                        None,
                    );
                }
            }
        }
    }

    // -------------------------------------------------------------------- watch

    fn watch_due(&mut self, factory_id: &str, event: bool) {
        let now = self.now();
        let Some(factory) = self.factories.get(factory_id).cloned() else {
            return;
        };
        if factory.closed || factory.paused || self.tasks_of(factory_id).next().is_none() {
            return;
        }
        let interval_due = factory
            .watch_last_at
            .is_none_or(|at| now.saturating_sub(at) >= factory.config.watch_interval_ms);
        if !event && !interval_due {
            return;
        }
        let board = serde_json::to_value(summary::build(
            &[&factory],
            &self.tasks_of(factory_id).collect::<Vec<_>>(),
            now,
            self.ports.clock.utc_offset_ms(),
            &self.runtime_blocked,
        ))
        .unwrap_or_default();
        let judgment = Judgment {
            id: format!("{factory_id}:watch:{now}"),
            factory: factory_id.to_owned(),
            task: None,
            priority: Priority::Factory,
            input: JudgmentInput::Watch { board },
            ai: None,
        };
        if let Some(f) = self.factories.get_mut(factory_id) {
            f.watch_last_at = Some(now);
        }
        self.save_factory(factory_id);
        if self.submit_judgment(judgment.clone()).is_ok() {
            self.judgments.insert(
                judgment.id,
                (factory_id.to_owned(), None, Purpose::Watch { read_at: now }),
            );
        }
        if interval_due {
            self.periodic_checks(&factory, now);
        }
    }

    /// The periodic user checks run on the watch cadence over each running
    /// Task's card (B67); one that cannot be queued is logged, since nothing
    /// waits on it.
    fn periodic_checks(&mut self, factory: &Factory, now: UnixMs) {
        let checks: Vec<String> = factory
            .config
            .checks
            .iter()
            .filter(|c| c.at == CheckPoint::Periodic)
            .map(|c| c.instruction.clone())
            .collect();
        if checks.is_empty() {
            return;
        }
        let running: Vec<(String, Card)> = self
            .tasks_of(&factory.id)
            .filter(|t| t.state == TaskState::Running)
            .map(|t| (t.id.clone(), t.card.clone()))
            .collect();
        for (id, card) in running {
            for (index, instruction) in checks.iter().enumerate() {
                let judgment = Judgment {
                    id: format!("{}:{id}:periodic:{index}:{now}", factory.id),
                    factory: factory.id.clone(),
                    task: Some(id.clone()),
                    priority: Priority::Factory,
                    input: JudgmentInput::Check {
                        instruction: instruction.clone(),
                        card: card.clone(),
                        diff: None,
                    },
                    ai: None,
                };
                match self.submit_judgment(judgment.clone()) {
                    Ok(()) => {
                        self.judgments.insert(
                            judgment.id,
                            (factory.id.clone(), Some(id.clone()), Purpose::Periodic),
                        );
                    }
                    Err(failure) => self.record(
                        &factory.id,
                        Some(&id),
                        "check.skipped",
                        json!({"at": "periodic", "stage": failure.stage}),
                    ),
                }
            }
        }
    }

    fn apply_watch(&mut self, factory_id: &str, value: &Value, read_at: UnixMs) {
        let Ok(warnings) = judgment::parse_watch(value) else {
            return;
        };
        let now = self.now();
        let today = now / DAY_MS;
        for warning in warnings {
            let Some(action) = warning.action.clone() else {
                // No action to take: the log only (B69).
                self.record(
                    factory_id,
                    warning.task.as_deref(),
                    "watch.logged",
                    json!({}),
                );
                continue;
            };
            let (limit, sent) = {
                let Some(f) = self.factories.get_mut(factory_id) else {
                    return;
                };
                if f.watch_day != today {
                    f.watch_day = today;
                    f.watch_sent_today = 0;
                }
                (f.config.watch_daily_limit, f.watch_sent_today)
            };
            if sent >= limit {
                self.record(factory_id, None, "watch.capped", json!({}));
                continue;
            }
            let anchor = match &warning.task {
                // A warning about a Task this Factory does not have is about
                // nothing a person can act on here.
                Some(reference) => match self.resolve_within(Some(factory_id), reference) {
                    Ok((_, id)) => Some(id),
                    Err(_) => {
                        self.record(
                            factory_id,
                            None,
                            "watch.logged",
                            json!({"unresolved": true}),
                        );
                        continue;
                    }
                },
                None => self.tasks_of(factory_id).last().map(|t| t.id.clone()),
            };
            let Some(anchor) = anchor else { continue };
            // The board was read before the judgment answered; a Task that
            // moved since is no longer what the warning describes.
            if warning.task.is_some()
                && self
                    .task(factory_id, &anchor)
                    .is_some_and(|t| t.state_since > read_at)
            {
                self.record(
                    factory_id,
                    Some(&anchor),
                    "watch.logged",
                    json!({"stale": true}),
                );
                continue;
            }
            // A Task already waiting on a person is in the inbox; a warning
            // about it would say the same thing twice (design #13).
            if warning.task.is_some()
                && self
                    .task(factory_id, &anchor)
                    .is_some_and(|t| t.open_questions().next().is_some())
            {
                self.record(
                    factory_id,
                    Some(&anchor),
                    "watch.logged",
                    json!({"already_open": true}),
                );
                continue;
            }
            self.add_question(
                factory_id,
                &anchor,
                QuestionOrigin::Engine,
                QuestionKind::Notice,
                &format!("감시: {} (할 일: {action})", warning.text),
                &action,
                None,
                None,
                vec!["ok".into()],
                None,
            );
            if let Some(f) = self.factories.get_mut(factory_id) {
                f.watch_sent_today += 1;
            }
            self.save_factory(factory_id);
        }
    }

    // --------------------------------------------------------------- retention

    /// Cancelled Tasks, and Tasks an outside pull request took, past their
    /// keep period lose their worktree and local branch (D-58, B50).
    fn retention(&mut self) {
        let now = self.now();
        let expired: Vec<(String, String)> = self
            .all_tasks()
            .filter(|t| kept_for_revive(t) && !t.purged)
            .filter(|t| {
                let keep = self
                    .factories
                    .get(&t.factory)
                    .map_or(7 * DAY_MS, |f| f.config.cancel_keep_ms);
                t.cancelled_at
                    .is_some_and(|at| now.saturating_sub(at) > keep)
            })
            .map(|t| (t.factory.clone(), t.id.clone()))
            .collect();
        for (factory, id) in expired {
            if let Some(worker) = self.task(&factory, &id).and_then(|t| t.worker.clone())
                && let Err(failure) = self
                    .ports
                    .workers
                    .remove_worktree(&worker, Removal::Discarded)
            {
                self.cleanup_failed(&factory, &id, &failure);
                continue;
            }
            self.with_task(&factory, &id, |t| t.purged = true);
            self.record(&factory, Some(&id), "cleanup.cancelled", json!({}));
        }
    }
}

/// An open question a person answers before the Task merges.
/// A `--worker <n>` number as a place in the Factory's candidate list.
fn worker_index(factory: &Factory, worker: Option<usize>) -> Result<Option<usize>, Refusal> {
    let Some(number) = worker else {
        return Ok(None);
    };
    let count = factory.config.candidates().len();
    if number == 0 || number > count {
        return Err(refuse(
            "worker_out_of_range",
            format!("Choose a worker candidate from 1 to {count}"),
        )
        .with(json!({"min": 1, "max": count})));
    }
    Ok(Some(number - 1))
}

fn waits_on_answer(task: &Task) -> bool {
    task.open_questions().any(|q| {
        !matches!(
            q.kind,
            QuestionKind::Notice | QuestionKind::Default | QuestionKind::ScopeChange { .. }
        )
    })
}

/// A Task whose worktree waits out the keep period: cancelled, taken over
/// by an outside pull request, or done through one (its worker's own work was
/// never merged).
fn kept_for_revive(task: &Task) -> bool {
    match task.state {
        TaskState::Cancelled | TaskState::Outside => true,
        TaskState::Done => task.cancelled_at.is_some(),
        _ => false,
    }
}

fn with_letter(command: Command, letter: &str) -> Option<Command> {
    let letter = Some(letter.to_owned());
    Some(match command {
        Command::Ask {
            text,
            suggestion,
            default_action,
            deadline_hours,
            choices,
            ..
        } => Command::Ask {
            text,
            suggestion,
            default_action,
            deadline_hours,
            letter,
            choices,
        },
        Command::Block {
            text,
            suggestion,
            deadline_hours,
            choices,
            ..
        } => Command::Block {
            text,
            suggestion,
            deadline_hours,
            letter,
            choices,
        },
        Command::Propose {
            class,
            text,
            card,
            autonomy,
            reclassify,
            ..
        } => Command::Propose {
            class,
            text,
            card,
            autonomy,
            reclassify,
            letter,
        },
        Command::Done {
            summary, breaking, ..
        } => Command::Done {
            summary,
            breaking,
            letter,
        },
        Command::Decide { text } => Command::Decide { text },
        Command::Dep {
            task,
            on,
            remove: false,
        } => Command::Dep {
            task,
            on,
            remove: false,
        },
        _ => return None,
    })
}

/// A letter from a harness that follows the Observer protocol only (B25).
fn plain_letter(letter: &Inbound) -> Option<Command> {
    let body = letter.body.trim();
    let suggestion = body
        .lines()
        .find_map(|line| {
            let lower = line.to_lowercase();
            ["recommendation:", "suggestion:", "추천:", "제안:"]
                .iter()
                .find(|prefix| lower.trim_start().starts_with(*prefix))
                .map(|prefix| line.trim_start()[prefix.len()..].trim().to_owned())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(질문 본문을 보세요)".to_owned());
    match letter.kind.as_str() {
        "block" | "request" => Some(Command::Block {
            text: judgment::cut(body, TEXT_LIMIT),
            suggestion,
            deadline_hours: Some(24),
            letter: Some(letter.id.clone()),
            choices: Vec::new(),
        }),
        "report" => Some(Command::Done {
            summary: Some(judgment::cut(body, TEXT_LIMIT)),
            breaking: false,
            letter: Some(letter.id.clone()),
        }),
        _ => None,
    }
}

fn parse_signal(value: &str) -> Option<EnvSignal> {
    [
        EnvSignal::DiskFull,
        EnvSignal::OutOfMemory,
        EnvSignal::GithubAuth,
        EnvSignal::GithubForbidden,
        EnvSignal::GithubRateLimit,
        EnvSignal::GithubServer,
        EnvSignal::Network,
        EnvSignal::HerdrSocket,
        EnvSignal::UsageLimit,
    ]
    .into_iter()
    .find(|signal| signal.as_str() == value)
}

pub fn parse_issue(text: &str, source: SourceKind) -> Option<IssueRef> {
    let text = text.trim();
    if let Some(number) = text.strip_prefix("L-").and_then(|n| n.parse().ok()) {
        return Some(IssueRef::Local { number });
    }
    let number: u64 = text.trim_start_matches('#').parse().ok()?;
    match source {
        SourceKind::Github => Some(IssueRef::Github { number }),
        SourceKind::Local => u32::try_from(number)
            .ok()
            .map(|number| IssueRef::Local { number }),
    }
}

fn source_name(source: SourceKind) -> &'static str {
    match source {
        SourceKind::Github => "github",
        SourceKind::Local => "local",
    }
}

fn extract_criteria(body: &str) -> Vec<String> {
    body.lines()
        .map(str::trim)
        .filter_map(|line| {
            line.strip_prefix("- [ ]")
                .or_else(|| line.strip_prefix("- [x]"))
        })
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .take(LIST_LIMIT)
        .collect()
}

/// The marker that lets an issue write converge on retry (B73).
pub fn task_marker(factory: &Factory, task: &Task) -> String {
    format!("<!-- hide-factory: {}/{} -->", factory.id, task.id)
}

pub fn issue_body(task: &Task, factory: &Factory) -> String {
    let mut body = format!(
        "{}\n\n## 목표\n{}\n\n## 완료 조건\n",
        task_marker(factory, task),
        task.card.goal
    );
    for criterion in &task.card.criteria {
        body.push_str(&format!("- [ ] {criterion}\n"));
    }
    if !task.card.out_of_scope.is_empty() {
        body.push_str("\n## 범위 밖\n");
        for item in &task.card.out_of_scope {
            body.push_str(&format!("- {item}\n"));
        }
    }
    if factory.config.prd_in_issue
        && let Some(text) = task
            .attachments
            .last()
            .and_then(|a| std::fs::read_to_string(&a.path).ok())
    {
        body.push_str("\n## PRD\n");
        body.push_str(&judgment::cut(&text, 48 * 1024));
    }
    body
}

/// A command's caller as the host saw it.
#[derive(Clone, Copy, Debug)]
pub struct Caller<'a> {
    pub pane: Option<&'a str>,
    pub cwd: Option<&'a str>,
    /// Another pane the caller named, which the host could not check against
    /// its credential: it can only make the caller a worker, never an operator.
    pub claimed: Option<&'a str>,
    /// The agents above the caller in the spawn lineage, nearest first.
    pub ancestor_agents: &'a [String],
    /// The pane each of those agents was registered on.
    pub ancestor_panes: &'a [String],
    /// The lineage walk reached an agent with no parent.
    pub lineage_complete: bool,
    /// A Factory's own agent is in the lineage.
    pub factory_spawned: bool,
}

pub fn pr_body(task: &Task, factory: &Factory) -> String {
    let mut body = issue_body(task, factory);
    if !task.decisions.is_empty() {
        // A worker wrote these: in a code block, a "Fixes #N" in them is
        // text, never a closing reference to another Task's issue (D-33).
        let text: String = task
            .decisions
            .iter()
            .map(|decision| format!("- {}\n", decision.text))
            .collect();
        let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
        let fence = "`".repeat(longest.max(2) + 1);
        body.push_str(&format!("\n## 결정 기록\n{fence}text\n{text}{fence}\n"));
    }
    match &task.issue {
        Some(IssueRef::Github { number }) => body.push_str(&format!("\nCloses #{number}\n")),
        Some(IssueRef::Local { number }) => body.push_str(&format!("\nCloses L-{number}\n")),
        None => {}
    }
    body
}

fn worker_name(factory: &Factory, task: &Task) -> String {
    let name: String = factory
        .project_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(16)
        .collect();
    format!(
        "factory-{}-{}",
        if name.is_empty() { "p" } else { &name },
        task.id.to_ascii_lowercase()
    )
}

/// The card as a worker reads it: goal, criteria and what is out of scope.
fn card_text(card: &Card) -> String {
    let mut text = format!("목표:\n{}\n\n완료 조건:\n", card.goal);
    for criterion in &card.criteria {
        text.push_str(&format!("- {criterion}\n"));
    }
    if !card.out_of_scope.is_empty() {
        text.push_str("\n범위 밖 (하지 마세요):\n");
        for item in &card.out_of_scope {
            text.push_str(&format!("- {item}\n"));
        }
    }
    text
}

/// The worker's first prompt (B22): the card, the attachments, the harness
/// preset and the Factory's reporting rules.
pub fn worker_prompt(task: &Task, factory: &Factory, resumed: bool, hide: &str) -> String {
    let mut prompt = String::new();
    if resumed {
        prompt.push_str("Factory: 같은 worktree에서 이 Task를 이어서 맡습니다. 지금까지 한 일을 확인하고 이어가세요.\n\n");
    }
    prompt.push_str(&format!(
        "당신은 Software Factory의 worker입니다. Task {}: {}\n\n",
        task.display_id(),
        task.card.title
    ));
    prompt.push_str(&card_text(&task.card));
    for attachment in &task.attachments {
        prompt.push_str(&format!("\n첨부 (읽기 전용): {}\n", attachment.path));
    }
    if let Some(harness) = &factory.config.harness {
        prompt.push_str(&format!(
            "\n일하는 방식 ({}):\n{}\n",
            harness.name, harness.instructions
        ));
    }
    prompt.push_str(REPORTING_RULES);
    if hide != "hide" {
        prompt.push_str(&format!(
            "- 위와 이후 편지의 `hide`는 모두 이 프로그램입니다: '{}'\n",
            hide.replace('\'', "'\\''")
        ));
    }
    prompt
}

pub const REPORTING_RULES: &str = "
Factory 보고 규약 (반드시 지키세요):
- 이 worktree의 branch에 커밋하세요. main에 직접 push하거나 머지하지 마세요. 검증과 머지는 Factory가 합니다.
- 끝나면: hide factory done --summary '<무엇을 했는지>' [--breaking]  (바로 '검증 중'이 오면 차례를 끝내세요. 결과는 편지로 옵니다)
- 사람에게 화면으로 묻지 마세요. 물을 것은 모두 아래 ask나 block으로 보내세요. 화면의 질문은 아무도 읽지 않습니다.
- 기본 행동으로 계속할 수 있는 질문: hide factory ask --question '<질문>' --suggestion '<제안>' --default '<그동안 할 행동>' --deadline-hours 24 [--choice '<선택지>']...
- 답 없이는 진행할 수 없는 질문: hide factory block --question '<질문>' --suggestion '<제안>' --deadline-hours 24 [--choice '<선택지>']...  (차례를 끝내세요)
  선택지는 5개까지, 하나에 120자까지입니다.
- 작업 중 발견: hide factory propose --class in-scope|decision|scope-change|prerequisite|unrelated --text '<내용>'
  범위를 스스로 넓히지 마세요. 선행 작업은 prerequisite로 제안하고(--title --goal --criterion), 무관한 발견은 unrelated로 남기세요.
- 보고 없이 차례를 끝내면 Task가 멈춥니다.
";
