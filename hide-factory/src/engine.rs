//! The engine: every Factory on this machine, their Tasks and the rules that
//! move them (D-01, D-28, D-29). It is one value driven from one thread:
//! `command` for a caller, `letter` for mail addressed to a Factory, and
//! `tick` for time and the outside world. Each change is saved before the
//! answer leaves (B72).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};

use crate::adapters::{
    Clock, EnvSignal, Environment, Failure, Judge, MainCheck, MemoryPressure, MergeTarget,
    Notifier, OutsideEvent, PreMerge, RevertRef, TaskSource, Verifier, VerifyPoll, VerifyRun,
    WorkerRuntime, WorkerSpawn, WorkerStatus,
};
use crate::command::{CardInput, Command, Refusal, VerificationChoice};
use crate::dag;
use crate::judgment::{self, Judgment, JudgmentInput, JudgmentOutcome, OtherTask, Priority};
use crate::model::*;
use crate::role::{ROLE_NOT_ALLOWED, Role};
use crate::store::{Event, Record, Store, StoreError, sha256_hex};
use crate::summary::{self, FactorySummary};

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
const START_NOTICE_MS: u64 = 10 * MINUTE_MS;

struct VerifyState {
    run: VerifyRun,
    stage: AttemptStage,
}

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
    processed: BTreeSet<String>,
    processed_order: Vec<String>,
    /// Recent environment and Task failures the cascade rules read.
    env_failures: Vec<FailureNote>,
    /// New starts halted by a cascade until this time (B60).
    halt_until: Option<UnixMs>,
    env_problem_since: Option<UnixMs>,
    env_diagnosed: bool,
    hold_checked_at: Option<UnixMs>,
    hold_reason: Option<String>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Purpose {
    Intake,
    Drift,
    Check,
    /// A periodic user check on a running Task: it adds questions or marks
    /// and never holds a merge (B67).
    Periodic,
    Watch,
    Env,
}

type Reply = Result<Value, Refusal>;

fn refuse(reason: &str, next: &str) -> Refusal {
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
        summary::build(&factories, &tasks, self.now())
    }

    pub fn events(&self, factory: &str, task: Option<&str>, limit: usize) -> Vec<Event> {
        self.store.events(factory, task, limit).unwrap_or_default()
    }

    /// The role of a caller (D-33): a pane the Factory spawned, or a cwd
    /// inside a Factory worktree, is a worker of that Task.
    pub fn role_for(&self, pane: Option<&str>, cwd: Option<&str>) -> Option<(String, String)> {
        for task in self.all_tasks() {
            let Some(worker) = &task.worker else { continue };
            if matches!(task.state, TaskState::Cancelled | TaskState::Done) {
                continue;
            }
            if pane.is_some() && worker.pane.as_deref() == pane {
                return Some((task.factory.clone(), task.id.clone()));
            }
            if let Some(cwd) = cwd {
                let root = Path::new(&worker.worktree);
                if Path::new(cwd).starts_with(root) {
                    return Some((task.factory.clone(), task.id.clone()));
                }
            }
        }
        None
    }

    /// Worker panes and worktrees, for the daemon's role registry.
    pub fn worker_bindings(&self) -> Vec<(String, String, Option<String>, String)> {
        self.all_tasks()
            .filter(|task| !matches!(task.state, TaskState::Cancelled | TaskState::Done))
            .filter_map(|task| {
                task.worker.as_ref().map(|worker| {
                    (
                        task.factory.clone(),
                        task.id.clone(),
                        worker.pane.clone(),
                        worker.worktree.clone(),
                    )
                })
            })
            .collect()
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
        // A failed diagnostic write never changes a decision.
        let _ = self.store.append_event(&event);
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
            self.record(
                factory,
                task,
                "store.failed",
                json!({"stage": "record", "error": error.0}),
            );
        }
    }

    /// Every judgment's input is kept before it is queued (D-58).
    fn submit_judgment(&mut self, judgment: Judgment) -> Result<(), Failure> {
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
                }
            }
        });
        if let Some(from) = from {
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
        let notify = matches!(kind, QuestionKind::Blocking);
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
        };
        self.with_task(factory, id, |task| task.questions.push(question));
        self.record(
            factory,
            Some(id),
            "question.added",
            json!({"question": question_id}),
        );
        if notify
            && self
                .factories
                .get(factory)
                .is_some_and(|f| f.config.macos_notifications)
        {
            let title = self
                .task(factory, id)
                .map(|task| format!("Factory: {} needs an answer", task.display_id()))
                .unwrap_or_default();
            self.ports.notifier.macos(&title, "Open hide factory inbox");
        }
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
                Ok(json!({"count": summary.my_turn, "items": summary.inbox}))
            }
            Command::Answer {
                task,
                question,
                choice,
                text,
            } => {
                let (factory, id) = self.resolve(role, &task)?;
                self.answer(role, &factory, &id, question.as_deref(), choice, text)
            }
            Command::Ask {
                text,
                suggestion,
                default_action,
                deadline_hours,
                letter,
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
                )
            }
            Command::Block {
                text,
                suggestion,
                deadline_hours,
                letter,
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
                Ok(self.task_answer(&factory, &id, "paused; slot released"))
            }
            Command::Resume { task } => {
                let (factory, id) = self.resolve(role, &task)?;
                self.allowed(&factory, &id, "resume")?;
                self.set_state(&factory, &id, TaskState::Waiting);
                Ok(self.task_answer(&factory, &id, "resumes when a slot is free"))
            }
            Command::Retry { task } => {
                let (factory, id) = self.resolve(role, &task)?;
                self.allowed(&factory, &id, "retry")?;
                self.with_task(&factory, &id, |task| {
                    task.failures = 0;
                    task.environment_failures = 0;
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
        if let Role::Worker { factory, .. } = role {
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
        let writes = self.ports.source.planned_writes(&probe);
        let verification = match verification {
            Some(VerificationChoice::Ci { checks }) => {
                if !probe.github {
                    return Err(refuse(
                        "ci_unavailable",
                        "A project without GitHub verifies with --verify <command>",
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
            (!verification.exists()).then_some("검증이 없으면 auto를 쓸 수 없습니다");
        let requested_mode = merge_mode.unwrap_or(if verification.exists() {
            MergeMode::Auto
        } else {
            MergeMode::Manual
        });
        if requested_mode == MergeMode::Auto && !verification.exists() {
            return Err(refuse(
                "auto_needs_verification",
                "Choose --ci or --verify, or use --merge manual",
            )
            .with(json!({"candidates": candidates})));
        }
        if !confirm {
            return Ok(json!({
                "preview": true,
                "project": project,
                "source": if probe.github { "github" } else { "local" },
                "candidates": candidates,
                "merge_mode": "auto",
                "auto_unavailable": auto_unavailable,
                "default_runtime": probe.runtimes.first().copied().unwrap_or(Runtime::Claude),
                "writes": writes,
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
        };
        if let Err(failure) = self.ports.source.prepare(&factory) {
            return Err(self.github_refusal(&failure, "init"));
        }
        self.factories.insert(id.clone(), factory);
        self.tasks.entry(id.clone()).or_default();
        self.save_factory(&id);
        self.record(
            &id,
            None,
            "factory.created",
            json!({"source": source_name(source)}),
        );
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
                    &format!("Run gh auth refresh -s {scope}, then retry"),
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
            open_decisions: card.open_decisions.clone(),
            depends_on: depends,
            external: card.external.clone(),
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
        let card = self.validate_card(&factory_id, Some(&id), &input, None)?;
        let mut task = Task::draft(&factory_id, &id, seq, card, now);
        task.issue = issue_ref;
        task.human = HumanFields {
            review_directly: input.review_directly,
            priority: input.priority.unwrap_or(0),
            merge_mode: input.merge_mode,
            runtime: input.runtime,
        };
        task.producer_pane = producer_pane;
        if let Some(prd) = &input.prd {
            let attachment = self
                .store
                .attach(&factory_id, &id, Path::new(prd), 1)
                .map_err(|error| {
                    refuse("attachment_failed", "Check the PRD path")
                        .with(json!({"error": error.0}))
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
        let card = self.validate_card(factory_id, Some(id), &input, Some(&task.card))?;
        let attachment = match &input.prd {
            Some(prd) => {
                let version = task.attachments.last().map_or(1, |a| a.version + 1);
                let attachment = self
                    .store
                    .attach(factory_id, id, Path::new(prd), version)
                    .map_err(|error| {
                        refuse("attachment_failed", "Check the PRD path")
                            .with(json!({"error": error.0}))
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
        let (repo_files, guide) = repo_context(&project);
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
            },
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
            self.add_question(
                factory,
                id,
                QuestionOrigin::Engine,
                QuestionKind::Action,
                "검토를 하지 못했습니다. Settings › Background AI에서 provider를 고친 뒤 다시 검토하세요.",
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

    fn apply_intake(&mut self, factory: &str, id: &str, value: &Value) {
        let verdict = match judgment::parse_intake(value) {
            Ok(verdict) => verdict,
            Err(reason) => return self.review_failed(factory, id, &reason),
        };
        let result = verdict.result();
        let now = self.now();
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
                    self.with_task(factory_id, id, |task| task.issue = Some(issue));
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

    fn answer(
        &mut self,
        role: &Role,
        factory: &str,
        id: &str,
        question: Option<&str>,
        choice: Option<String>,
        text: Option<String>,
    ) -> Reply {
        let task = self
            .task(factory, id)
            .cloned()
            .ok_or_else(|| refuse("task_not_found", "Check hide factory status"))?;
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
        let now = self.now();
        let relayed_by = role.relayed_by();
        let answer = Answer {
            text: judgment::cut(&text, TEXT_LIMIT),
            chose: chosen.clone(),
            relayed_by: relayed_by.clone(),
            at: now,
        };
        self.with_task(factory, id, |task| {
            if let Some(question) = task.questions.iter_mut().find(|q| q.id == target.id) {
                question.answer = Some(answer.clone());
            }
            task.decisions.push(DecisionRecord {
                text: format!("{} -> {}", judgment::cut(&target.text, 200), answer.text),
                by: relayed_by.clone(),
                at: now,
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
                    let body = format!(
                        "Factory: 사람이 기본 행동과 다르게 답했습니다: {text}\n이 답을 반영한 뒤 다시 hide factory done 하세요."
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
                    self.reverts.remove(factory);
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
            QuestionKind::Proposal { .. } => {
                // A person runs a proposed command themselves; the Factory
                // runs only its typed recovery actions (D-54).
                self.record(
                    factory,
                    Some(id),
                    "proposal.answered",
                    json!({"decision": decision}),
                );
            }
            QuestionKind::Notice => {
                if task.state == TaskState::Done {
                    self.with_task(factory, id, |task| task.seen = true);
                }
            }
        }
        Ok(self.task_answer(factory, id, "answered"))
    }

    fn split(&mut self, factory: &str, id: &str, pieces: Vec<SplitPiece>) {
        let Some(first) = pieces.first().cloned() else {
            return;
        };
        // The original becomes the first piece and keeps its issue (D-55).
        self.with_task(factory, id, |task| {
            task.card.title = first.title.clone();
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
    ) -> Reply {
        let task = self
            .task(factory, id)
            .cloned()
            .ok_or_else(|| refuse("task_not_found", "Check hide factory status"))?;
        if text.trim().is_empty() {
            return Err(refuse("question_required", "Give --question"));
        }
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
            Vec::new(),
            letter,
        );
        self.with_task(factory, id, |task| task.last_report_at = Some(now));
        if blocking {
            // A question it cannot work past releases the slot and sleeps (B27).
            self.put_to_sleep(factory, id);
            self.set_state(factory, id, TaskState::Blocked);
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
            if let Some(text) = &summary_text {
                task.decisions.push(DecisionRecord {
                    text: format!("done: {}", judgment::cut(text, TEXT_LIMIT)),
                    by: format!("worker:{id}"),
                    at: now,
                });
            }
            task.gates.clear();
        });
        let _ = letter;
        self.put_to_sleep(factory_id, id);
        // B36: a pull request for a GitHub Factory before CI is read.
        if factory.source == SourceKind::Github && task.pr.is_none() {
            let body = pr_body(&task, &factory);
            match self.ports.merge.open_pr(&factory, &task, &body) {
                Ok(pr) => {
                    self.with_task(factory_id, id, |task| {
                        task.pr = pr;
                        task.writes.insert("pr".into());
                    });
                    self.record(factory_id, Some(id), "github.pr_opened", json!({}));
                }
                Err(failure) => {
                    self.external_failure(factory_id, Some(id), &failure);
                }
            }
        }
        self.set_state(factory_id, id, TaskState::Verifying);
        self.start_checks(factory_id, id);
        if !factory.config.verification.exists() {
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
        let diff = self.ports.merge.diff_text(&f, &task).unwrap_or_default();
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
        };
        match self.submit_judgment(drift.clone()) {
            Ok(()) => {
                self.judgments.insert(
                    drift.id,
                    (factory.to_owned(), Some(id.to_owned()), Purpose::Drift),
                );
                count += 1;
            }
            Err(_) => self.check_failed(factory, id),
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
            };
            match self.submit_judgment(judgment.clone()) {
                Ok(()) => {
                    self.judgments.insert(
                        judgment.id,
                        (factory.to_owned(), Some(id.to_owned()), Purpose::Check),
                    );
                    count += 1;
                }
                Err(_) => self.check_failed(factory, id),
            }
        }
        self.checks_running
            .insert((factory.to_owned(), id.to_owned()), count);
    }

    /// A check that could not run is never skipped: merge waits for a person (B68).
    fn check_failed(&mut self, factory: &str, id: &str) {
        self.with_task(factory, id, |task| {
            if !task.gates.contains(&Gate::CheckFailed) {
                task.gates.push(Gate::CheckFailed);
            }
        });
        self.record(factory, Some(id), "check.failed", json!({}));
    }

    fn apply_finding(&mut self, factory: &str, id: &str, value: &Value) {
        let finding = match judgment::parse_finding(value) {
            Ok(finding) => finding,
            Err(_) => return self.check_failed(factory, id),
        };
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
        self.start_checks(factory, id);
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
        let key = (factory_id.to_owned(), id.to_owned());
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
        // Merge-tree and the quick check, in seconds (B38).
        if factory.config.verification.exists() || factory.source == SourceKind::Local {
            match self.ports.merge.premerge(&factory, &task) {
                Ok(PreMerge::Clean) => {}
                Ok(PreMerge::Conflict { files }) => {
                    // A rebase is not a verification failure.
                    self.record(
                        factory_id,
                        Some(id),
                        "merge.conflict",
                        json!({"files": files.len()}),
                    );
                    self.set_state(factory_id, id, TaskState::Running);
                    self.wake(factory_id, id, &format!(
                        "Factory: 최신 main과 충돌합니다 ({}). main 위로 rebase한 뒤 다시 hide factory done 하세요. 이 일은 검증 실패로 세지 않습니다.",
                        files.join(", ")
                    ));
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
        if task.human.review_directly {
            gates.push(Gate::ReviewDirectly);
        }
        if task.scope_approved {
            gates.push(Gate::ApprovedScopeChange);
        }
        if task.breaking {
            gates.push(Gate::BreakingChange);
        }
        if !factory.config.verification.exists() {
            gates.push(Gate::NoVerification);
        }
        if mode == MergeMode::Manual {
            gates.push(Gate::ManualMode);
        }
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
        if task.open_questions().any(|q| {
            !matches!(
                q.kind,
                QuestionKind::Notice | QuestionKind::Default | QuestionKind::ScopeChange { .. }
            )
        }) {
            gates.push(Gate::OpenQuestion);
        }
        gates.dedup();
        if !gates.is_empty() {
            self.with_task(factory_id, id, |task| task.gates = gates.clone());
            self.set_state(factory_id, id, TaskState::MergeWaiting);
            return;
        }
        if factory.main.broken {
            // Auto merge is stopped until main is green again (B44, D-47).
            return;
        }
        self.merge_now(factory_id, id);
    }

    fn merge_now(&mut self, factory_id: &str, id: &str) -> Option<String> {
        let factory = self.factories.get(factory_id).cloned()?;
        let task = self.task(factory_id, id).cloned()?;
        if factory.source == SourceKind::Local {
            match self.ports.merge.main_dirty(&factory) {
                Ok(true) => {
                    self.with_task(factory_id, id, |task| {
                        if !task.gates.contains(&Gate::DirtyMain) {
                            task.gates.push(Gate::DirtyMain);
                        }
                    });
                    self.set_state(factory_id, id, TaskState::MergeWaiting);
                    return None;
                }
                Ok(false) => {}
                Err(failure) => {
                    self.external_failure(factory_id, Some(id), &failure);
                    return None;
                }
            }
        }
        match self
            .ports
            .merge
            .merge(&factory, &task, factory.config.merge_method)
        {
            Ok(sha) => {
                let now = self.now();
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
                Some(sha)
            }
            Err(failure) => {
                self.external_failure(factory_id, Some(id), &failure);
                None
            }
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
        match self.merge_now(factory_id, id) {
            Some(sha) => Ok(
                json!({"message": "merged", "sha": sha, "task": self.task_answer(factory_id, id, "")["task"]}),
            ),
            None => Err(refuse(
                "merge_failed",
                "See hide factory show for the reason",
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

    /// A finished Task's worker ends and its worktree goes; the branch stays
    /// with the merge. A failure is logged and the disk recovery retries it.
    fn release_worker(&mut self, factory_id: &str, id: &str) {
        let Some(worker) = self.task(factory_id, id).and_then(|t| t.worker.clone()) else {
            return;
        };
        let _ = self.ports.workers.stop(&worker);
        match self.ports.workers.remove_worktree(&worker, false) {
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
        if factory.config.macos_notifications {
            self.ports.notifier.macos(
                "Factory: main is broken",
                "Auto merge stopped. Open hide factory inbox",
            );
        }
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
        self.reverts
            .insert(factory_id.to_owned(), RevertPhase::Bisecting { index: 0 });
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
                            self.reverts
                                .insert(factory_id.to_owned(), RevertPhase::Bisecting { index });
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
                                self.reverts.remove(factory_id);
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
                self.reverts.insert(
                    factory_id.to_owned(),
                    RevertPhase::Reverting {
                        revert,
                        task: task.to_owned(),
                    },
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

    fn revert_needs_person(&mut self, factory_id: &str, why: &str, merges: &[LandedMerge]) {
        if let Some(f) = self.factories.get_mut(factory_id) {
            f.main.needs_person = true;
        }
        self.save_factory(factory_id);
        self.reverts.remove(factory_id);
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
                    task.idle_since = None;
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
            let _ = self.ports.workers.stop(worker);
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
        let (_, on_id) = self.resolve(role, on)?;
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
            .filter(|t| t.state.column() == Some(Column::Running))
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
                    config.verification = Verification::Ci {
                        checks: value
                            .split(',')
                            .map(str::trim)
                            .filter(|c| !c.is_empty())
                            .map(str::to_owned)
                            .collect(),
                    };
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
                    config.default_runtime = Runtime::parse(value).ok_or_else(bad)?
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
            if config.merge_mode == MergeMode::Auto && !config.verification.exists() {
                return Err(refuse(
                    "auto_needs_verification",
                    "Set a verification before auto merge",
                ));
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

    fn status(&self, project: Option<&str>) -> Reply {
        let mut summary = self.summary();
        if let Some(project) = project {
            let id = self.factory_id(Some(project))?;
            summary.factories.retain(|f| f.id == id);
        }
        Ok(serde_json::to_value(&summary).unwrap_or_default())
    }

    /// A Task's kept judgment and letter bodies, oldest first (D-58).
    pub fn records(&self, factory: &str, id: &str, limit: usize) -> Vec<Record> {
        self.store.records(factory, id, limit).unwrap_or_default()
    }

    pub fn show(&self, factory: &str, id: &str) -> Option<summary::TaskDetail> {
        let factory = self.factories.get(factory)?;
        let tasks = self.tasks.get(&factory.id)?;
        let task = tasks.get(id)?;
        Some(summary::detail(
            factory,
            task,
            tasks,
            Self::allowed_actions(task),
            self.now(),
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
            let _ = self.store.set_meta(
                "processed_letters",
                &serde_json::to_string(&self.processed_order).unwrap_or_default(),
            );
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
                            self.apply_finding(&factory, &task, value)
                        }
                        JudgmentOutcome::Failed { .. } if drafting => {}
                        JudgmentOutcome::Failed { .. } => self.check_failed(&factory, &task),
                    }
                    self.advance_merge(&factory, &task);
                }
                (JudgmentOutcome::Answered { value }, Purpose::Periodic, Some(task)) => {
                    match judgment::parse_finding(value) {
                        Ok(_) => self.apply_finding(&factory, &task, value),
                        Err(_) => self.record(
                            &factory,
                            Some(&task),
                            "judgment.failed",
                            json!({"purpose": "Periodic", "reason": "unreadable"}),
                        ),
                    }
                }
                (JudgmentOutcome::Answered { value }, Purpose::Watch, _) => {
                    self.apply_watch(&factory, value)
                }
                (JudgmentOutcome::Answered { value }, Purpose::Env, _) => {
                    self.apply_diagnosis(&factory, value)
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
        let running: Vec<(String, String, WorkerRef, Option<UnixMs>, UnixMs)> = self
            .all_tasks()
            .filter(|t| t.state == TaskState::Running)
            .filter_map(|t| {
                t.worker.clone().map(|w| {
                    (
                        t.factory.clone(),
                        t.id.clone(),
                        w,
                        t.last_report_at,
                        t.state_since,
                    )
                })
            })
            .collect();
        for (factory, id, worker, last_report, since) in running {
            let no_report = self
                .factories
                .get(&factory)
                .map_or(2 * MINUTE_MS, |f| f.config.no_report_ms);
            match self.ports.workers.status(&worker) {
                WorkerStatus::Resting { since: rest } => {
                    // A turn that ended with no Factory report since it began (B24).
                    let reported = last_report.is_some_and(|at| at >= rest.min(since).max(since));
                    if !reported && now.saturating_sub(rest) >= no_report && rest >= since {
                        // A worker its usage limit stopped waits for a slot
                        // and is not the Task's fault (B58).
                        if let Some(until) = self.ports.workers.usage_limited(worker.runtime) {
                            let mut failure = Failure::environment(
                                "worker",
                                EnvSignal::UsageLimit,
                                "usage limit",
                            );
                            failure.reset_at = Some(until);
                            self.external_failure(&factory, Some(&id), &failure);
                            continue;
                        }
                        self.with_task(&factory, &id, |task| {
                            task.stop = Some(StopReason::NoReport)
                        });
                        self.set_state(&factory, &id, TaskState::Stopped);
                        self.with_task(&factory, &id, |task| {
                            task.stop = Some(StopReason::NoReport)
                        });
                        self.record(&factory, Some(&id), "worker.no_report", json!({}));
                    }
                }
                WorkerStatus::Gone => {
                    self.with_task(&factory, &id, |task| task.stop = Some(StopReason::NoReport));
                    self.set_state(&factory, &id, TaskState::Stopped);
                    self.with_task(&factory, &id, |task| task.stop = Some(StopReason::NoReport));
                    self.record(&factory, Some(&id), "worker.gone", json!({}));
                }
                WorkerStatus::Working | WorkerStatus::Blocked => {}
            }
        }
    }

    fn start_tasks(&mut self) {
        let now = self.now();
        // A Task cancelled or paused while its worker was starting frees
        // the slot.
        let starting: Vec<_> = self.starting.keys().cloned().collect();
        for (factory, id) in starting {
            if self
                .task(&factory, &id)
                .is_none_or(|t| t.state != TaskState::Waiting)
            {
                self.starting.remove(&(factory, id));
            }
        }
        if self.halt_until.is_some_and(|until| until > now) {
            return;
        }
        // Rule 1: hold starts below the disk floor or at critical memory
        // pressure; re-checked when a worker ends or a minute passes (B57).
        let due = self
            .hold_checked_at
            .is_none_or(|at| now.saturating_sub(at) >= ENV_RECHECK_MS)
            || self.hold_reason.is_none();
        let mut candidates: Vec<Task> = self
            .all_tasks()
            .filter(|t| self.factories.get(&t.factory).is_some_and(|f| !f.closed))
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
                self.hold_reason = Some("디스크 여유가 기준보다 작음".into());
            } else if self.ports.environment.memory_pressure() == MemoryPressure::Critical {
                self.hold_reason = Some("메모리 압박 critical".into());
            }
        }
        if let Some(reason) = self.hold_reason.clone() {
            self.env_problem_since.get_or_insert(now);
            for task in &candidates {
                if task.held.as_deref() != Some(reason.as_str()) {
                    let held = reason.clone();
                    self.with_task(&task.factory, &task.id, |t| t.held = Some(held));
                }
            }
            return;
        }
        for task in &candidates {
            if task.held.is_some() {
                self.with_task(&task.factory, &task.id, |t| t.held = None);
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
        for runtime in [Runtime::Claude, Runtime::Codex] {
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
        let mut runtime = task.runtime(&factory);
        if let Some(until) = self.runtime_blocked.get(&runtime).copied() {
            if until > now {
                let other = match runtime {
                    Runtime::Claude => Runtime::Codex,
                    Runtime::Codex => Runtime::Claude,
                };
                if task.human.runtime.is_none()
                    && self.runtime_blocked.get(&other).is_none_or(|u| *u <= now)
                {
                    runtime = other;
                } else {
                    return false;
                }
            } else {
                self.runtime_blocked.remove(&runtime);
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
                    args: factory
                        .config
                        .worker_args
                        .get(runtime.as_str())
                        .cloned()
                        .unwrap_or_default(),
                    resume: Some(worker.clone()),
                    attempt: task.spawn_refusals,
                };
                self.ports.workers.spawn(&request).map(Some)
            } else {
                self.ports.workers.wake(&worker, &body).map(|()| None)
            };
            match restarted {
                Ok(new_worker) => {
                    self.with_task(factory_id, id, |t| {
                        if let Some(w) = new_worker {
                            t.worker = Some(w);
                        }
                        if let Some(w) = &mut t.worker {
                            w.asleep = false;
                        }
                        t.flags.retain(|f| !f.starts_with("pending reply: "));
                        t.last_report_at = None;
                    });
                    self.set_state(factory_id, id, TaskState::Running);
                    true
                }
                Err(failure) => {
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
                args: factory
                    .config
                    .worker_args
                    .get(runtime.as_str())
                    .cloned()
                    .unwrap_or_default(),
                resume: None,
                attempt: task.spawn_refusals,
            };
            let key = (factory_id.to_owned(), id.to_owned());
            match self.ports.workers.spawn(&request) {
                Ok(worker) => {
                    self.starting.remove(&key);
                    self.with_task(factory_id, id, |t| {
                        t.worker = Some(worker);
                        t.last_report_at = None;
                    });
                    self.record(
                        factory_id,
                        Some(id),
                        "worker.started",
                        json!({"runtime": runtime.as_str()}),
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
        self.starting.insert(key, (first, now + START_RETRY_MS));
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
                        self.set_state(factory_id, &task.id, TaskState::Done);
                        self.release_worker(factory_id, &task.id);
                    }
                    TaskState::Outside => {}
                    TaskState::Running
                    | TaskState::Verifying
                    | TaskState::Blocked
                    | TaskState::Paused
                    | TaskState::MergeWaiting
                    | TaskState::Stopped => {
                        // Stop the worker; the worktree stays 7 days (B50).
                        self.cancel_verification(factory_id, &task.id);
                        if let Some(worker) = &task.worker {
                            let _ = self.ports.workers.stop(worker);
                        }
                        self.with_task(factory_id, &task.id, |t| {
                            t.cancelled_at = Some(t.updated_at);
                            t.cancelled_from = Some(t.state);
                            if let Some(w) = &mut t.worker {
                                w.asleep = true;
                            }
                        });
                        self.set_state(factory_id, &task.id, TaskState::Outside);
                        self.notice(factory_id, &task.id, &format!("밖의 PR이 이 Task의 issue를 닫습니다: {url}. worker를 멈췄고 worktree는 7일 남습니다. 되살리려면 hide factory revive {}", task.display_id()));
                    }
                    _ => {
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
                    // Its outside PR closed the issue: follow it to done.
                    self.set_state(factory_id, &task.id, TaskState::Done);
                    self.release_worker(factory_id, &task.id);
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
            OutsideEvent::BodyEdited { issue, body_hash } => {
                let Some(task) = self.task_for_issue(factory_id, &issue) else {
                    return;
                };
                if task.source_body_hash.as_deref() == Some(body_hash.as_str()) {
                    return;
                }
                let first = task.source_body_hash.is_none();
                self.with_task(factory_id, &task.id, |t| {
                    t.source_body_hash = Some(body_hash.clone())
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
                    t.source_body_hash = Some(sha256_hex(body.as_bytes()));
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
            let due = factory.main.broken
                || factory
                    .outside_read_at
                    .is_some_and(|at| self.now().saturating_sub(at) < 1000)
                || !self.main_seen.contains_key(&factory.id);
            if !due {
                continue;
            }
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
                Ok(MainCheck::Red { link }) if !known && !factory.main.broken => {
                    self.main_seen.insert(factory.id.clone(), head.clone());
                    self.main_broken(&factory.id, &head, &link);
                }
                Ok(_) => {
                    if !factory.main.broken {
                        self.main_seen.insert(factory.id.clone(), head);
                    }
                }
                Err(failure) => self.external_failure(&factory.id, None, &failure),
            }
        }
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
                self.hold_reason = Some("디스크 부족".into());
                self.hold_checked_at = Some(now);
                self.remove_finished_worktrees();
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
    /// their keep period (B62).
    fn remove_finished_worktrees(&mut self) {
        let now = self.now();
        let targets: Vec<(String, String, WorkerRef)> = self
            .all_tasks()
            .filter(|t| !t.purged)
            .filter(|t| {
                let keep = self
                    .factories
                    .get(&t.factory)
                    .map_or(7 * DAY_MS, |f| f.config.cancel_keep_ms);
                t.state == TaskState::Done
                    || (matches!(t.state, TaskState::Cancelled | TaskState::Outside)
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
            let finished = self
                .task(&factory, &id)
                .is_some_and(|t| t.state == TaskState::Done);
            match self.ports.workers.remove_worktree(&worker, !finished) {
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
            "hold": self.hold_reason,
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
        };
        if self.submit_judgment(judgment.clone()).is_ok() {
            self.judgments
                .insert(judgment.id, (factory.id.clone(), None, Purpose::Env));
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
                self.record(
                    factory,
                    None,
                    "recovery.run",
                    json!({"action": action.as_str()}),
                );
                match action {
                    RecoveryAction::RemoveFinishedWorktrees => self.remove_finished_worktrees(),
                    RecoveryAction::RetryReadsAndReconnect => {
                        self.github_backoff.clear();
                        self.hold_reason = None;
                    }
                    RecoveryAction::RestartWorker
                    | RecoveryAction::SleepWakeWorker
                    | RecoveryAction::SwitchRuntime => {
                        self.runtime_blocked.clear();
                    }
                }
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
        if factory.closed || self.tasks_of(factory_id).next().is_none() {
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
        ))
        .unwrap_or_default();
        let judgment = Judgment {
            id: format!("{factory_id}:watch:{now}"),
            factory: factory_id.to_owned(),
            task: None,
            priority: Priority::Factory,
            input: JudgmentInput::Watch { board },
        };
        if let Some(f) = self.factories.get_mut(factory_id) {
            f.watch_last_at = Some(now);
        }
        self.save_factory(factory_id);
        if self.submit_judgment(judgment.clone()).is_ok() {
            self.judgments
                .insert(judgment.id, (factory_id.to_owned(), None, Purpose::Watch));
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

    fn apply_watch(&mut self, factory_id: &str, value: &Value) {
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
            let anchor = warning
                .task
                .clone()
                .and_then(|t| self.resolve(&Role::Engine, &t).ok().map(|(_, id)| id))
                .or_else(|| self.tasks_of(factory_id).last().map(|t| t.id.clone()));
            let Some(anchor) = anchor else { continue };
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
            .filter(|t| matches!(t.state, TaskState::Cancelled | TaskState::Outside) && !t.purged)
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
                && let Err(failure) = self.ports.workers.remove_worktree(&worker, true)
            {
                self.cleanup_failed(&factory, &id, &failure);
                continue;
            }
            self.with_task(&factory, &id, |t| t.purged = true);
            self.record(&factory, Some(&id), "cleanup.cancelled", json!({}));
        }
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
            ..
        } => Command::Ask {
            text,
            suggestion,
            default_action,
            deadline_hours,
            letter,
        },
        Command::Block {
            text,
            suggestion,
            deadline_hours,
            ..
        } => Command::Block {
            text,
            suggestion,
            deadline_hours,
            letter,
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

fn repo_context(project: &str) -> (Vec<String>, Option<String>) {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(project) {
        for entry in entries.flatten().take(200) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && name != ".github" {
                continue;
            }
            files.push(name);
        }
    }
    files.sort();
    let guide = ["AGENTS.md", "CLAUDE.md", "README.md"]
        .iter()
        .find_map(|name| std::fs::read_to_string(Path::new(project).join(name)).ok())
        .map(|text| judgment::cut(&text, 8 * 1024));
    (files, guide)
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

pub fn pr_body(task: &Task, factory: &Factory) -> String {
    let mut body = issue_body(task, factory);
    if !task.decisions.is_empty() {
        body.push_str("\n## 결정 기록\n");
        for decision in &task.decisions {
            body.push_str(&format!("- {}\n", decision.text));
        }
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
- 기본 행동으로 계속할 수 있는 질문: hide factory ask --question '<질문>' --suggestion '<제안>' --default '<그동안 할 행동>' --deadline-hours 24
- 답 없이는 진행할 수 없는 질문: hide factory block --question '<질문>' --suggestion '<제안>' --deadline-hours 24  (차례를 끝내세요)
- 작업 중 발견: hide factory propose --class in-scope|decision|scope-change|prerequisite|unrelated --text '<내용>'
  범위를 스스로 넓히지 마세요. 선행 작업은 prerequisite로 제안하고(--title --goal --criterion), 무관한 발견은 unrelated로 남기세요.
- 보고 없이 차례를 끝내면 Task가 멈춥니다.
";
