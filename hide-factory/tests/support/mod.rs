//! One fake world behind every adapter the engine drives. Each fake records
//! the calls that would reach GitHub, git or a worker, so a test asserts what
//! was written and how many times, and answers from scripted queues.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use hide_factory::adapters::*;
use hide_factory::command::{CardInput, Command, VerificationChoice};
use hide_factory::judgment::{Judgment, JudgmentAnswer, JudgmentInput, JudgmentOutcome};
use hide_factory::model::*;
use hide_factory::role::Role;
use hide_factory::{Engine, Ports};
use serde_json::{Value, json};
use tempfile::TempDir;

pub const PROJECT: &str = "/work/fixture";

#[derive(Default)]
pub struct World {
    pub now: UnixMs,
    pub github: bool,
    /// The GitHub login `gh api user` answers; `None` when logged out.
    pub account: Option<String>,
    /// Every external write, in order: `verb target`.
    pub writes: Vec<String>,
    pub next_issue: u64,
    pub next_pr: u64,
    pub next_sha: u64,
    /// Scripted verification results by Task id; `Passed` when empty.
    pub verify: BTreeMap<String, VecDeque<VerifyPoll>>,
    pub verify_start_failure: Option<Failure>,
    pub verify_runs: Vec<String>,
    pub cancelled_runs: Vec<String>,
    pub premerge: BTreeMap<String, VecDeque<PreMerge>>,
    pub premerge_calls: u32,
    /// The next merge is refused with this failure.
    pub merge_refusal: Option<Failure>,
    /// This many merges answer before GitHub names the merge commit.
    pub merge_unnamed: u32,
    /// Reads of whether a merge GitHub answered has landed.
    pub merged_reads: u32,
    /// The pull request turns out not to have merged (a queue dropped it).
    pub merge_dropped: bool,
    /// The next read of whether a merge landed fails with this failure.
    pub merged_read_failure: Option<Failure>,
    pub merge_attempts: u32,
    /// Every push is refused with this failure.
    pub publish_refusal: Option<Failure>,
    /// Every merge fails with this failure.
    pub merge_failure: Option<Failure>,
    /// Each report a GitHub Task pushed, by Task id.
    pub pushes: Vec<String>,
    /// Main verification answers by commit, read before `main_checks`.
    pub main_check_script: BTreeMap<String, VecDeque<MainCheck>>,
    /// Main verification by commit; `Green` when absent.
    pub main_checks: BTreeMap<String, MainCheck>,
    pub head: String,
    pub main_dirty: bool,
    pub revert_check: Option<MainCheck>,
    pub diff_lines: u32,
    /// Worker status by Task id; `Working` when absent.
    pub worker_status: BTreeMap<String, WorkerStatus>,
    pub spawned: Vec<WorkerSpawn>,
    pub messages: Vec<(String, String)>,
    pub sleeps: Vec<String>,
    pub wakes: Vec<(String, String)>,
    pub stops: Vec<String>,
    pub removed: Vec<String>,
    /// Worktrees holding uncommitted leftovers, by Task id.
    pub dirty_worktrees: BTreeSet<String>,
    pub branches_deleted: Vec<String>,
    /// Runtimes the probe finds installed; none when empty.
    pub runtimes: Vec<Runtime>,
    /// Usage-limit resets the machine reports by runtime.
    pub usage_limits: BTreeMap<Runtime, UnixMs>,
    pub spawn_failure: Option<Failure>,
    /// Every spawn the engine asked for, answered or not.
    pub spawn_asks: Vec<WorkerSpawn>,
    /// Starts the engine gave up on, by Task id.
    pub abandoned: Vec<String>,
    /// Judgments submitted and not yet answered.
    pub submitted: Vec<Judgment>,
    pub judged: Vec<Judgment>,
    /// Intake answers by card title; a plain ready verdict when absent.
    pub intake: BTreeMap<String, Value>,
    pub drift: BTreeMap<String, Value>,
    /// Scripted watch answers, oldest first; no warnings when empty.
    pub watch: VecDeque<Value>,
    pub judge_down: bool,
    /// How many times the free disk was read.
    pub disk_reads: u32,
    /// The Task's diff cannot be read.
    pub diff_failure: bool,
    /// Every judgment answers as failed with this reason.
    pub judgment_failure: Option<String>,
    /// The environment diagnosis's answer; an unknown cause when absent.
    pub env_diagnosis: Option<Value>,
    pub hold_judgments: bool,
    pub disk_free: Option<u64>,
    pub memory: Option<MemoryPressure>,
    pub outside: VecDeque<OutsideEvent>,
    pub observe_failure: Option<Failure>,
    pub observed: u32,
    pub macos: Vec<String>,
    pub producer: Vec<(String, String)>,
}

#[derive(Clone)]
pub struct Shared(pub Arc<Mutex<World>>);

impl Shared {
    pub fn world(&self) -> MutexGuard<'_, World> {
        self.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }
}

impl Clock for Shared {
    fn now(&self) -> UnixMs {
        self.world().now
    }

    /// The fixtures keep their days in UTC.
    fn utc_offset_ms(&self) -> i64 {
        0
    }
}

impl TaskSource for Shared {
    fn probe(&mut self, _project: &str) -> Result<ProjectProbe, Failure> {
        let github = self.world().github;
        Ok(ProjectProbe {
            github,
            repo: github.then(|| "owner/fixture".into()),
            account: github.then(|| self.world().account.clone()).flatten(),
            required_checks: if github {
                vec!["test".into()]
            } else {
                Vec::new()
            },
            verify_candidates: vec!["cargo test".into()],
            merge_methods: vec![MergeMethod::Squash],
            default_branch: "main".into(),
            runtimes: self.world().runtimes.clone(),
        })
    }
    fn prepare(&mut self, factory: &Factory) -> Result<(), Failure> {
        if factory.source == SourceKind::Github {
            self.world().writes.push("label.create factory".into());
        }
        Ok(())
    }
    fn create_issue(
        &mut self,
        factory: &Factory,
        task: &Task,
        _body: &str,
    ) -> Result<IssueRef, Failure> {
        let mut world = self.world();
        world.next_issue += 1;
        let number = world.next_issue;
        world.writes.push(format!("issue.create {}", task.id));
        Ok(match factory.source {
            SourceKind::Github => IssueRef::Github {
                number: 100 + number,
            },
            SourceKind::Local => IssueRef::Local {
                number: number as u32,
            },
        })
    }
    fn label_issue(&mut self, _factory: &Factory, issue: &IssueRef) -> Result<(), Failure> {
        self.world()
            .writes
            .push(format!("issue.label {}", issue.display()));
        Ok(())
    }
    fn read_issue(&mut self, _factory: &Factory, issue: &IssueRef) -> Result<IssueText, Failure> {
        Ok(IssueText {
            title: format!("Issue {}", issue.display()),
            body: "- [ ] it works".into(),
            open: true,
        })
    }
    fn observe(
        &mut self,
        _factory: &Factory,
        _tasks: &[&Task],
    ) -> Result<Vec<OutsideEvent>, Failure> {
        let mut world = self.world();
        world.observed += 1;
        if let Some(failure) = world.observe_failure.clone() {
            return Err(failure);
        }
        Ok(world.outside.drain(..).collect())
    }
}

impl Verifier for Shared {
    fn start(&mut self, _factory: &Factory, task: &Task) -> Result<VerifyRun, Failure> {
        let mut world = self.world();
        if let Some(failure) = world.verify_start_failure.take() {
            return Err(failure);
        }
        let id = format!("{}:{}", task.id, world.verify_runs.len());
        world.verify_runs.push(id.clone());
        Ok(VerifyRun {
            id,
            log: None,
            commit: None,
        })
    }
    fn start_premerge(&mut self, factory: &Factory, task: &Task) -> Result<VerifyRun, Failure> {
        self.start(factory, task)
    }
    fn poll(&mut self, _factory: &Factory, run: &VerifyRun) -> VerifyPoll {
        let task = run.id.split(':').next().unwrap_or_default().to_owned();
        self.world()
            .verify
            .get_mut(&task)
            .and_then(VecDeque::pop_front)
            .unwrap_or(VerifyPoll::Passed)
    }
    fn cancel(&mut self, run: &VerifyRun) {
        self.world().cancelled_runs.push(run.id.clone());
    }
}

impl MergeTarget for Shared {
    fn main_head(&mut self, _factory: &Factory) -> Result<String, Failure> {
        Ok(self.world().head.clone())
    }
    fn open_pr(
        &mut self,
        _factory: &Factory,
        task: &Task,
        _body: &str,
    ) -> Result<Option<PullRequest>, Failure> {
        let mut world = self.world();
        world.pushes.push(task.id.clone());
        if let Some(failure) = world.publish_refusal.clone() {
            return Err(failure);
        }
        if let Some(pr) = task.pr.clone().filter(|pr| pr.open) {
            return Ok(Some(pr));
        }
        world.next_pr += 1;
        let number = world.next_pr;
        world.writes.push(format!("pr.open {}", task.id));
        Ok(Some(PullRequest {
            number,
            url: format!("https://github.example/pr/{number}"),
            head: task.branch_slug(),
            by_factory: true,
            open: true,
        }))
    }
    fn close_pr(&mut self, _factory: &Factory, pr: &PullRequest) -> Result<(), Failure> {
        self.world().writes.push(format!("pr.close {}", pr.number));
        Ok(())
    }
    fn reopen_pr(&mut self, _factory: &Factory, pr: &PullRequest) -> Result<(), Failure> {
        self.world().writes.push(format!("pr.reopen {}", pr.number));
        Ok(())
    }
    fn diff_lines(&mut self, _factory: &Factory, _task: &Task) -> Result<u32, Failure> {
        Ok(self.world().diff_lines)
    }
    fn changed_paths(&mut self, _factory: &Factory, _task: &Task) -> Result<Vec<String>, Failure> {
        Ok(vec!["src/lib.rs".into()])
    }
    fn diff_text(&mut self, _factory: &Factory, task: &Task) -> Result<String, Failure> {
        if self.world().diff_failure {
            return Err(Failure::task("git.diff", "unreadable"));
        }
        Ok(format!("diff for {}", task.id))
    }
    fn premerge(&mut self, _factory: &Factory, task: &Task) -> Result<PreMerge, Failure> {
        self.world().premerge_calls += 1;
        Ok(self
            .world()
            .premerge
            .get_mut(&task.id)
            .and_then(VecDeque::pop_front)
            .unwrap_or(PreMerge::Clean))
    }
    fn main_dirty(&mut self, _factory: &Factory) -> Result<bool, Failure> {
        Ok(self.world().main_dirty)
    }
    fn merge(
        &mut self,
        _factory: &Factory,
        task: &Task,
        _method: MergeMethod,
    ) -> Result<String, Failure> {
        let mut world = self.world();
        world.merge_attempts += 1;
        if let Some(failure) = world.merge_refusal.take() {
            return Err(failure);
        }
        if let Some(failure) = world.merge_failure.clone() {
            return Err(failure);
        }
        if world.merge_unnamed > 0 {
            world.merge_unnamed -= 1;
            return Err(Failure {
                again_in_ms: Some(hide_factory::engine::MERGE_COMMIT_AGAIN_MS),
                ..Failure::task("github.merge", "merge commit not named yet")
            });
        }
        world.next_sha += 1;
        let sha = format!("sha{:04}-{}", world.next_sha, task.id);
        world.writes.push(format!("merge {}", task.id));
        world.head = sha.clone();
        Ok(sha)
    }
    fn merged_commit(
        &mut self,
        _factory: &Factory,
        task: &Task,
    ) -> Result<Option<String>, Failure> {
        let mut world = self.world();
        world.merged_reads += 1;
        if let Some(failure) = world.merged_read_failure.take() {
            return Err(failure);
        }
        if world.merge_dropped {
            world.merge_dropped = false;
            return Ok(None);
        }
        if world.merge_unnamed > 0 {
            world.merge_unnamed -= 1;
            return Err(Failure {
                again_in_ms: Some(hide_factory::engine::MERGE_COMMIT_AGAIN_MS),
                ..Failure::task("github.merged", "merge commit not named yet")
            });
        }
        world.next_sha += 1;
        let sha = format!("sha{:04}-{}", world.next_sha, task.id);
        world.writes.push(format!("merge {}", task.id));
        world.head = sha.clone();
        Ok(Some(sha))
    }
    fn main_check(&mut self, _factory: &Factory, sha: &str) -> Result<MainCheck, Failure> {
        if let Some(next) = self
            .world()
            .main_check_script
            .get_mut(sha)
            .and_then(VecDeque::pop_front)
        {
            return Ok(next);
        }
        Ok(self
            .world()
            .main_checks
            .get(sha)
            .cloned()
            .unwrap_or(MainCheck::Green))
    }
    fn rerun_main(&mut self, _factory: &Factory, sha: &str) -> Result<(), Failure> {
        self.world().writes.push(format!("main.rerun {sha}"));
        Ok(())
    }
    fn revert(&mut self, _factory: &Factory, task: &Task, sha: &str) -> Result<RevertRef, Failure> {
        self.world().writes.push(format!("revert.open {}", task.id));
        Ok(RevertRef {
            task: task.id.clone(),
            sha: sha.to_owned(),
            pr: Some(900),
            commit: None,
        })
    }
    fn revert_check(
        &mut self,
        _factory: &Factory,
        _revert: &RevertRef,
    ) -> Result<MainCheck, Failure> {
        Ok(self
            .world()
            .revert_check
            .clone()
            .unwrap_or(MainCheck::Green))
    }
    fn merge_revert(&mut self, _factory: &Factory, revert: &RevertRef) -> Result<String, Failure> {
        let mut world = self.world();
        world.next_sha += 1;
        let sha = format!("sha{:04}-revert-{}", world.next_sha, revert.task);
        world.writes.push(format!("revert.merge {}", revert.task));
        world.head = sha.clone();
        Ok(sha)
    }
}

impl WorkerRuntime for Shared {
    fn spawn(&mut self, request: &WorkerSpawn) -> Result<WorkerRef, Failure> {
        let mut world = self.world();
        world.spawn_asks.push(request.clone());
        if let Some(failure) = world.spawn_failure.clone() {
            return Err(failure);
        }
        world.spawned.push(request.clone());
        world.worker_status.remove(&request.task);
        Ok(WorkerRef {
            factory: String::new(),
            agent: Some(format!("agent-{}", request.task)),
            name: request.name.clone(),
            pane: Some(format!("pane-{}", request.task)),
            runtime: request.runtime,
            worktree: format!("/work/fixture.worktrees/{}", request.task),
            branch: request.branch.clone(),
            started_at: world.now,
            asleep: false,
        })
    }
    fn message(
        &mut self,
        worker: &WorkerRef,
        _intent: &str,
        _reply_to: Option<&str>,
        body: &str,
    ) -> Result<(), Failure> {
        self.world()
            .messages
            .push((task_of(worker), body.to_owned()));
        Ok(())
    }
    fn sleep(&mut self, worker: &WorkerRef) -> Result<(), Failure> {
        self.world().sleeps.push(task_of(worker));
        Ok(())
    }
    fn wake(&mut self, worker: &WorkerRef, body: &str) -> Result<(), Failure> {
        let mut world = self.world();
        world.wakes.push((task_of(worker), body.to_owned()));
        world.worker_status.remove(&task_of(worker));
        Ok(())
    }
    fn status(&mut self, worker: &WorkerRef) -> WorkerStatus {
        self.world()
            .worker_status
            .get(&task_of(worker))
            .copied()
            .unwrap_or(WorkerStatus::Working)
    }
    fn abandon_start(&mut self, _factory: &str, task: &str) -> bool {
        let mut world = self.world();
        world.abandoned.push(task.to_owned());
        // A pending start is still in flight; a consumed `starting` answer
        // left nothing in the adapter's hands.
        world
            .spawn_failure
            .as_ref()
            .is_some_and(|failure| failure.again_in_ms == Some(0))
    }
    fn stop(&mut self, worker: &WorkerRef) -> Result<(), Failure> {
        self.world().stops.push(task_of(worker));
        Ok(())
    }
    fn remove_worktree(&mut self, worker: &WorkerRef, removal: Removal) -> Result<(), Failure> {
        let mut world = self.world();
        let task = task_of(worker);
        if removal == Removal::Finished && world.dirty_worktrees.contains(&task) {
            return Err(Failure::task(
                "worktree.remove",
                "the worktree contains modified or untracked files",
            ));
        }
        world.removed.push(task.clone());
        if removal == Removal::Discarded {
            world.branches_deleted.push(task);
        }
        Ok(())
    }
    fn usage_limited(&mut self, runtime: Runtime) -> Option<UnixMs> {
        self.world().usage_limits.get(&runtime).copied()
    }
}

fn task_of(worker: &WorkerRef) -> String {
    worker
        .worktree
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned()
}

impl Judge for Shared {
    fn submit(&mut self, judgment: Judgment) -> Result<(), Failure> {
        let mut world = self.world();
        if world.judge_down {
            return Err(Failure::task("judge", "no provider"));
        }
        world.submitted.push(judgment);
        Ok(())
    }
    fn finished(&mut self) -> Vec<JudgmentAnswer> {
        let mut world = self.world();
        if world.hold_judgments {
            return Vec::new();
        }
        let submitted = std::mem::take(&mut world.submitted);
        let mut answers = Vec::new();
        for judgment in submitted {
            if let Some(reason) = world.judgment_failure.clone() {
                answers.push(JudgmentAnswer {
                    id: judgment.id.clone(),
                    factory: judgment.factory.clone(),
                    task: judgment.task.clone(),
                    outcome: JudgmentOutcome::Failed { reason },
                });
                continue;
            }
            let value = match &judgment.input {
                JudgmentInput::IntakeReview { card, .. } => {
                    world.intake.get(&card.title).cloned().unwrap_or_else(
                        || json!({"questions": [], "dependencies": [], "flags": [], "split": [], "fits_scope": true}),
                    )
                }
                JudgmentInput::Drift { .. } | JudgmentInput::Check { .. } => judgment
                    .task
                    .as_ref()
                    .and_then(|task| world.drift.get(task).cloned())
                    .unwrap_or_else(|| json!({"pass": true, "questions": [], "flags": []})),
                JudgmentInput::Watch { .. } => world
                    .watch
                    .pop_front()
                    .unwrap_or_else(|| json!({"warnings": []})),
                JudgmentInput::EnvDiagnosis { .. } => world
                    .env_diagnosis
                    .clone()
                    .unwrap_or_else(|| json!({"cause": "unknown", "action": "none"})),
            };
            answers.push(JudgmentAnswer {
                id: judgment.id.clone(),
                factory: judgment.factory.clone(),
                task: judgment.task.clone(),
                outcome: JudgmentOutcome::Answered { value },
            });
            world.judged.push(judgment);
        }
        answers
    }
}

impl Environment for Shared {
    fn disk_free(&mut self, _project: &str) -> Option<u64> {
        self.world().disk_reads += 1;
        self.world().disk_free
    }
    fn memory_pressure(&mut self) -> MemoryPressure {
        self.world().memory.unwrap_or(MemoryPressure::Normal)
    }
}

impl Notifier for Shared {
    fn macos(&mut self, title: &str, _body: &str) {
        self.world().macos.push(title.to_owned());
    }
    fn producer(&mut self, _factory: &str, pane: &str, body: &str) -> bool {
        self.world()
            .producer
            .push((pane.to_owned(), body.to_owned()));
        true
    }
}

/// An engine over the fake world in a private state folder.
pub struct Bench {
    pub dir: TempDir,
    pub shared: Shared,
    pub engine: Engine,
}

pub fn operator() -> Role {
    Role::Operator {
        pane: "operator".into(),
    }
}

pub fn worker(factory: &str, task: &str) -> Role {
    Role::Worker {
        factory: factory.into(),
        task: task.into(),
    }
}

fn ports(shared: &Shared) -> Ports {
    Ports {
        clock: Box::new(shared.clone()),
        source: Box::new(shared.clone()),
        verifier: Box::new(shared.clone()),
        merge: Box::new(shared.clone()),
        workers: Box::new(shared.clone()),
        judge: Box::new(shared.clone()),
        environment: Box::new(shared.clone()),
        notifier: Box::new(shared.clone()),
    }
}

impl Bench {
    pub fn new(github: bool) -> Self {
        let dir = tempfile::tempdir().expect("state folder");
        let world = World {
            now: 1_000 * DAY_MS,
            account: Some("octo".into()),
            github,
            head: "sha0000-base".into(),
            diff_lines: 40,
            disk_free: Some(100 << 30),
            ..World::default()
        };
        let shared = Shared(Arc::new(Mutex::new(world)));
        let engine = Engine::open(
            &dir.path().join("factory.sqlite3"),
            &dir.path().join("factory-files"),
            ports(&shared),
        )
        .expect("engine opens");
        Self {
            dir,
            shared,
            engine,
        }
    }

    /// The same state folder and world, a new engine: a daemon restart.
    pub fn restart(self) -> Self {
        let Self {
            dir,
            shared,
            engine,
        } = self;
        drop(engine);
        let engine = Engine::open(
            &dir.path().join("factory.sqlite3"),
            &dir.path().join("factory-files"),
            ports(&shared),
        )
        .expect("engine reopens");
        Self {
            dir,
            shared,
            engine,
        }
    }

    pub fn world(&self) -> MutexGuard<'_, World> {
        self.shared.world()
    }

    pub fn advance(&mut self, ms: u64) {
        self.world().now += ms;
    }

    pub fn op(&mut self, command: Command) -> Value {
        self.engine.command(&operator(), command)
    }

    pub fn as_worker(&mut self, factory: &str, task: &str, command: Command) -> Value {
        self.engine.command(&worker(factory, task), command)
    }

    /// Creates a Factory; `verify` gives it a verify bundle and auto merge.
    pub fn factory(&mut self, verify: bool) -> String {
        self.factory_at(PROJECT, verify)
    }

    pub fn factory_at(&mut self, project: &str, verify: bool) -> String {
        let answer = self.op(Command::Init {
            project: project.into(),
            verification: Some(if verify {
                VerificationChoice::Commands {
                    commands: vec!["cargo test".into()],
                }
            } else {
                VerificationChoice::None
            }),
            merge_mode: Some(if verify {
                MergeMode::Auto
            } else {
                MergeMode::Manual
            }),
            confirm: true,
        });
        assert_eq!(answer["created"], true, "{answer}");
        answer["factory"]["id"]
            .as_str()
            .expect("factory id")
            .to_owned()
    }

    pub fn add(&mut self, title: &str, depends_on: &[&str]) -> Value {
        self.add_card(card(title, depends_on))
    }

    pub fn add_card(&mut self, card: CardInput) -> Value {
        self.op(Command::Add {
            project: Some(PROJECT.into()),
            task: None,
            issue: None,
            card,
            producer_pane: None,
        })
    }

    /// Adds a Task and lets its review finish; returns its id.
    pub fn ready(&mut self, title: &str, depends_on: &[&str]) -> String {
        let answer = self.add(title, depends_on);
        assert_eq!(answer["ok"], true, "{answer}");
        let id = answer["task"]["id"].as_str().expect("task id").to_owned();
        self.engine.tick();
        id
    }

    pub fn task(&self, factory: &str, id: &str) -> Task {
        self.engine.task(factory, id).cloned().expect("task exists")
    }

    pub fn state(&self, factory: &str, id: &str) -> TaskState {
        self.task(factory, id).state
    }

    pub fn done(&mut self, factory: &str, id: &str) -> Value {
        self.as_worker(
            factory,
            id,
            Command::Done {
                summary: Some(format!("did {id}")),
                breaking: false,
                letter: None,
            },
        )
    }

    pub fn writes(&self, prefix: &str) -> Vec<String> {
        self.world()
            .writes
            .iter()
            .filter(|write| write.starts_with(prefix))
            .cloned()
            .collect()
    }
}

pub fn card(title: &str, depends_on: &[&str]) -> CardInput {
    CardInput {
        title: Some(title.into()),
        goal: Some(format!("Make {title} work")),
        criteria: vec![format!("{title} passes its test")],
        depends_on: depends_on.iter().map(|id| (*id).to_owned()).collect(),
        ..CardInput::default()
    }
}
