//! `FactorySummary`: the one value the CLI's `status` and `inbox` print and
//! the screens draw (D-50). Every number in it is derived here from the
//! store's Tasks (design #4, #10); the shell computes nothing.
//!
//! The inbox is 결정 필요 (D-32): only what a person moves, questions,
//! merges, stops the recovery could not clear, a closed pane, and the
//! one-button to-dos. Everything else is the activity log.
//!
//! Board columns follow movement; engine lifecycle states remain unchanged.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::dag;
use crate::judgment::WorkerTextSource;
use crate::model::{
    Activity, Attachment, AttemptOutcome, AttemptStage, ChoiceOutcome, Column, CriterionState,
    CriterionVerdict, DAY_MS, DecisionChange, DecisionKind, DecisionSource, Discovery, EnvHold,
    Factory, FactoryAi, FollowUpState, Gate, GithubBlock, HoldKey, MergeMode, OBSERVER,
    PauseReason, PullRequest, Question, QuestionKind, RecoveryAttempt, Runtime, SourceKind,
    StopReason, Task, TaskState, UnixMs, Verification, WorkerCandidate, WorkerReport, decision_id,
};

/// What a card waits for, as a code beside `waiting_for`'s words, so a
/// screen can say it in any language (stage 2 B24).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitingFor {
    /// Tasks it depends on; `waiting_on` names them.
    Predecessors,
    /// A free worker slot.
    Slot,
    /// The machine holds new starts; `env_hold` says why.
    Environment,
    /// A person's answer to a blocking question.
    Answer,
}

impl WaitingFor {
    pub const ALL: [Self; 4] = [
        Self::Predecessors,
        Self::Slot,
        Self::Environment,
        Self::Answer,
    ];
}

/// What sending an inbox item's preselected answer does, as a code beside
/// `result`'s words; `unblocks` names the Tasks it frees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultCode {
    /// The sleeping worker wakes and continues.
    WakeWorker,
    /// An answer that differs from the default goes to the worker; the same
    /// one goes on to merge.
    ApplyOrMerge,
    /// With no question left the Task is Ready.
    Ready,
    /// The Task is split into the proposed pieces.
    Split,
    /// The approved Task enters drafting.
    Drafting,
    /// The chosen of split, continue or stop.
    NewTaskCapChoice,
    /// The chosen action runs.
    RunAction,
    Merge,
    /// The worker restarts in the same worktree.
    RestartWorker,
    /// A paused Task resumes in the same worktree.
    ResumeWorker,
    /// The to-do is marked done and what it held continues.
    Resolve,
}

impl ResultCode {
    pub const ALL: [Self; 11] = [
        Self::WakeWorker,
        Self::ApplyOrMerge,
        Self::Ready,
        Self::Split,
        Self::Drafting,
        Self::NewTaskCapChoice,
        Self::RunAction,
        Self::Merge,
        Self::RestartWorker,
        Self::ResumeWorker,
        Self::Resolve,
    ];
}

/// What a 결정 필요 item holds up, as a code a screen says in any language
/// when the asker did not write it (D-33).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Holding {
    /// A worker that sleeps until the answer.
    Worker,
    /// The Task's start.
    Start,
    /// The Task's merge.
    Merge,
    /// The Task's progress: it stopped.
    Progress,
    /// Every new start of the Factory.
    Starts,
    /// The Factory's GitHub steps.
    Github,
    /// Nothing: the work goes on with the default meanwhile.
    Continues,
}

impl Holding {
    pub const ALL: [Self; 7] = [
        Self::Worker,
        Self::Start,
        Self::Merge,
        Self::Progress,
        Self::Starts,
        Self::Github,
        Self::Continues,
    ];
}

/// Who made a decision on a Task page (D-35).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionBy {
    Person,
    Ai,
    Worker,
}

impl DecisionBy {
    pub const ALL: [Self; 3] = [Self::Person, Self::Ai, Self::Worker];
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactorySummary {
    /// The one person-facing number: every 결정 필요 item across Factories.
    pub my_turn: u32,
    pub factories: Vec<FactoryView>,
    pub inbox: Vec<InboxItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactoryView {
    pub id: String,
    pub project: String,
    pub project_name: String,
    pub source: String,
    /// `ci`, `verify` or `none` (D-21).
    pub verification: String,
    pub closed: bool,
    /// The operator paused the whole Factory (D-48).
    #[serde(default)]
    pub paused: bool,
    pub flow: Flow,
    pub my_turn: u32,
    /// `manual`, `assist` or `autonomous` (직접, 함께, 맡김).
    #[serde(default)]
    pub observer_mode: String,
    /// Observer calls today and the daily cap (D-34).
    #[serde(default)]
    pub observer_today: u32,
    #[serde(default)]
    pub observer_limit: u32,
    /// Today's Factory AI calls reached the cap; the header marks it (B21).
    #[serde(default)]
    pub observer_capped: bool,
    /// GitHub refused the Factory's sign-in or a permission (D-46).
    #[serde(default)]
    pub github_block: Option<GithubBlock>,
    /// Follow-up candidates still open, newest first (D-31).
    #[serde(default)]
    pub follow_ups: Vec<FollowUpView>,
    /// The Factory's activity, newest last, the latest [`FACTORY_ACTIVITY_SHOWN`].
    #[serde(default)]
    pub activity: Vec<Activity>,
    /// The last seven local days' three numbers (D-45).
    #[serde(default)]
    pub metrics: Metrics,
    /// The Factory AI; `None` is the app's Hide AI (D-40).
    pub factory_ai: Option<FactoryAi>,
    /// The worker candidates, the first the default (D-41).
    #[serde(default)]
    pub workers: Vec<WorkerCandidate>,
    #[serde(default)]
    pub macos_notifications: bool,
    pub columns: Vec<ColumnView>,
    /// Off the board, revivable for the keep period (D-47).
    pub cancelled: Vec<CardView>,
    pub graph: Graph,
    /// Every dependency edge `(predecessor, task)`; the graph's reduced
    /// edges are a display step over this set (D-36).
    pub dependencies: Vec<(String, String)>,
    pub outside_read_at: Option<UnixMs>,
    /// Three outside reads failed in a row (B65).
    pub stale: bool,
    pub main_broken: bool,
    pub auto_merge_available: bool,
    pub merge_mode: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flow {
    pub before: u32,
    pub moving: u32,
    pub stuck: u32,
    pub done_today: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnView {
    pub column: String,
    pub label: String,
    pub cards: Vec<CardView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardView {
    pub task: String,
    /// Factory AI's decisions that stand on this Task (B25).
    #[serde(default)]
    pub ai_decisions: u32,
    /// A GitHub step waits for the Factory's access (B33).
    #[serde(default)]
    pub permission_wait: bool,
    /// The recovery schedule is working on its stop (B15).
    #[serde(default)]
    pub recovering: bool,
    /// `T-n` before Ready, the issue number after.
    pub display_id: String,
    pub column: Option<String>,
    pub title: String,
    pub summary: String,
    pub issue: Option<String>,
    pub issue_url: Option<String>,
    pub pr: Option<PullRequest>,
    pub worker_runtime: Option<String>,
    /// The worker's agent as a person reads it, its accessible name (B43).
    pub worker_label: Option<String>,
    pub resume_at: Option<UnixMs>,
    /// `person` or `other`, only in the stuck column.
    pub waiting_group: Option<String>,
    /// Waiting, work, verify, merge, or all complete (0..=4).
    pub stage: u8,
    pub state: String,
    pub state_label: String,
    pub needs_person: bool,
    /// What a waiting Task waits for, in words (D-36).
    pub waiting_for: Option<String>,
    /// The same as a code.
    pub waiting_code: Option<WaitingFor>,
    /// The display ids of the predecessors it waits on.
    pub waiting_on: Vec<String>,
    /// Why the machine holds new starts, for `environment`.
    pub env_hold: Option<EnvHold>,
    /// The stop reason behind a stopped card's `state_label`.
    pub stop: Option<StopReason>,
    /// Why a paused card is paused.
    pub pause_reason: Option<PauseReason>,
    pub priority: i32,
    pub since: UnixMs,
    /// A completion the person has not looked at (D-30).
    pub unread: bool,
    /// A completion older than 3 days folds (D-47).
    pub folded: bool,
    /// A finished Task older than 90 days leaves the folded group too; its
    /// page still opens.
    pub archived: bool,
    pub failures: u32,
    /// Work in another repository it waits on (B64).
    pub external: Vec<String>,
    /// A cancelled Task can be revived until this time (B71).
    pub revive_until: Option<UnixMs>,
    /// The worker's pane, which stage 2 leaves out of the Overview counts.
    pub worker_pane: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<String>,
    /// Reduced edges `(predecessor, task)` (D-36); the data keeps all.
    pub edges: Vec<(String, String)>,
    /// Tasks with no relation, drawn below.
    pub unrelated: Vec<String>,
}

/// The follow-up candidates a Factory and a Task page list (D-31).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowUpView {
    /// The Task it came from.
    pub task: String,
    pub display_id: String,
    pub discovery: String,
    pub text: String,
    pub state: FollowUpState,
    pub issue: Option<String>,
    pub issue_url: Option<String>,
    /// The Task it became.
    pub became: Option<String>,
    /// Why the last attempt to make its issue failed (B19).
    pub failure: Option<String>,
    pub at: UnixMs,
}

/// Seven days of how much the Factory needed a person (D-45). A count of
/// zero leaves its number out, which a screen shows as '-'.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metrics {
    /// Tasks finished in the period, and the 결정 필요 items a person moved
    /// per finished Task, in tenths.
    pub finished: u32,
    pub person_items_tenths: Option<u32>,
    /// Tasks whose first worker started in the period, and the median time
    /// from the Task's creation (its label) to that start.
    pub started: u32,
    pub start_median_ms: Option<u64>,
    /// Factory AI's decisions in the period, the ones a person changed, and
    /// the percentage.
    pub ai_decisions: u32,
    pub overridden: u32,
    pub override_percent: Option<u32>,
}

/// One 결정 필요 item (D-33, D-49 order).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxItem {
    /// `answer`, `merge`, `stopped` or `todo`.
    pub group: String,
    /// The question kind, `merge`, `stopped`, `paused`, or a to-do's
    /// `github`, `command`, `start` or `hold`.
    pub kind: String,
    pub rank: u8,
    pub factory: String,
    /// The Task it is about; a Factory's to-do has none.
    pub task: Option<String>,
    pub display_id: Option<String>,
    pub title: String,
    pub project: String,
    pub question: Option<String>,
    /// The question or to-do in one sentence: the asker's words for a
    /// question, the stop's detail or the to-do's cause otherwise.
    pub text: String,
    /// What it holds up, as the asker wrote it.
    pub stopped: Option<String>,
    /// The same as a code, always set.
    pub holding: Holding,
    /// Each choice with what choosing it leads to, where the asker wrote it.
    pub outcomes: Vec<ChoiceOutcome>,
    /// Why Factory AI did not decide it: `failed`, `daily_limit`,
    /// `paused`, `queue_full`, `dropped`, or none when its kind is a
    /// person's (B7).
    pub fallback: Option<String>,
    /// What the item's evidence row unfolds: links, checks, the report.
    pub evidence: Vec<String>,
    /// The one button's item for `hide factory resolve`.
    pub resolve: Option<String>,
    /// A to-do's command to copy, and what running it does (B16).
    pub command: Option<String>,
    pub impact: Option<String>,
    /// The preselected proposal.
    pub suggestion: String,
    /// What sending the preselected answer does.
    pub result: String,
    pub default_action: Option<String>,
    pub choices: Vec<String>,
    pub deadline: Option<UnixMs>,
    pub remaining: Option<String>,
    /// Whole hours left before the deadline, rounded up; 0 once it passed.
    pub remaining_hours: Option<u64>,
    pub waiting_since: UnixMs,
    /// Whole days a blocking question has waited.
    pub waiting_days: u64,
    /// `result` as a code.
    pub result_code: ResultCode,
    /// The display ids of waiting Tasks this item frees once it is done.
    pub unblocks: Vec<String>,
    /// Why a merge item waits for a person.
    pub gates: Vec<Gate>,
    /// Why a stopped item stopped.
    pub stop: Option<StopReason>,
    /// Why the machine holds starts, for a hold to-do.
    pub env_hold: Option<EnvHold>,
    /// What the recovery schedule already tried, for a hold to-do or a
    /// stop it gave up on (B14).
    #[serde(default)]
    pub attempts: Vec<RecoveryAttempt>,
    /// The Observer's kind and reason for a request it sorted.
    pub decision_kind: Option<DecisionKind>,
    pub observer_reason: Option<String>,
}

/// How many of a Factory's latest activity lines the summary carries.
pub const FACTORY_ACTIVITY_SHOWN: usize = 50;
/// The period the metrics count (D-45).
const METRICS_DAYS: i64 = 7;

pub fn build(
    factories: &[&Factory],
    tasks: &[&Task],
    now: UnixMs,
    utc_offset_ms: i64,
    runtime_holds: &BTreeMap<Runtime, UnixMs>,
) -> FactorySummary {
    let mut summary = FactorySummary::default();
    for factory in factories {
        let mine: BTreeMap<String, Task> = tasks
            .iter()
            .filter(|task| task.factory == factory.id)
            .map(|task| (task.id.clone(), (*task).clone()))
            .collect();
        let items = inbox_items(factory, &mine, now);
        let view = factory_view(
            factory,
            &mine,
            items.len() as u32,
            now,
            utc_offset_ms,
            runtime_holds,
        );
        summary.my_turn += view.my_turn;
        summary.inbox.extend(items);
        summary.factories.push(view);
    }
    summary.inbox.sort_by(|a, b| {
        a.rank
            .cmp(&b.rank)
            .then(a.waiting_since.cmp(&b.waiting_since))
    });
    summary
}

/// The day a time falls on where the machine is, not in UTC: Seoul's day
/// starts at 15:00 UTC, so a Task done at 08:00 there is done today.
fn local_day(at: UnixMs, utc_offset_ms: i64) -> i64 {
    (at as i64 + utc_offset_ms).div_euclid(DAY_MS as i64)
}

fn factory_view(
    factory: &Factory,
    tasks: &BTreeMap<String, Task>,
    my_turn: u32,
    now: UnixMs,
    utc_offset_ms: i64,
    runtime_holds: &BTreeMap<Runtime, UnixMs>,
) -> FactoryView {
    let today = local_day(now, utc_offset_ms);
    let mut flow = Flow::default();
    let mut columns: BTreeMap<Column, Vec<(&Task, CardView)>> = BTreeMap::new();
    let mut cancelled = Vec::new();
    for task in tasks.values() {
        let mut card = card_view(factory, task, tasks, now);
        if task.state == TaskState::Waiting {
            let runtime = task
                .worker
                .as_ref()
                .map_or_else(|| task.candidate(factory).0.agent, |worker| worker.runtime);
            card.resume_at = runtime_holds
                .get(&runtime)
                .copied()
                .filter(|until| *until > now);
        }
        match board_column(task) {
            Some(Column::Before) => flow.before += 1,
            Some(Column::Moving) => flow.moving += 1,
            Some(Column::Stuck) => flow.stuck += 1,
            Some(Column::Done)
                if task
                    .done_at
                    .is_some_and(|at| local_day(at, utc_offset_ms) == today) =>
            {
                flow.done_today += 1;
            }
            Some(Column::Done) => {}
            None => {}
        }
        match board_column(task) {
            Some(column) => columns.entry(column).or_default().push((task, card)),
            None => cancelled.push(card),
        }
    }
    let columns = Column::ALL
        .into_iter()
        .map(|column| {
            let mut cards = columns.remove(&column).unwrap_or_default();
            // Person cards first, longest waiting first; then priority, then age (D-47).
            cards.sort_by(|(a, a_view), (b, b_view)| {
                let a_person =
                    a_view.waiting_group.as_deref() == Some("person") || a_view.needs_person;
                let b_person =
                    b_view.waiting_group.as_deref() == Some("person") || b_view.needs_person;
                b_person
                    .cmp(&a_person)
                    .then((b.state == TaskState::Stopped).cmp(&(a.state == TaskState::Stopped)))
                    .then_with(|| {
                        if a_person {
                            a.state_since.cmp(&b.state_since)
                        } else {
                            dag::slot_order(a, b)
                        }
                    })
            });
            ColumnView {
                column: column.as_str().to_owned(),
                label: column.label().to_owned(),
                cards: cards.into_iter().map(|(_, view)| view).collect(),
            }
        })
        .collect();
    let edges = dag::edges(tasks.values());
    let reduced = dag::transitive_reduction(&edges);
    let related: std::collections::BTreeSet<&String> =
        reduced.iter().flat_map(|(a, b)| [a, b]).collect();
    let graph = Graph {
        nodes: tasks
            .values()
            .filter(|task| task.state != TaskState::Cancelled)
            .map(|task| task.id.clone())
            .collect(),
        unrelated: tasks
            .values()
            .filter(|task| task.state != TaskState::Cancelled && !related.contains(&task.id))
            .map(|task| task.id.clone())
            .collect(),
        edges: reduced,
    };
    let dependencies = edges
        .iter()
        .flat_map(|(task, predecessors)| {
            predecessors
                .iter()
                .map(move |predecessor| (predecessor.clone(), task.clone()))
        })
        .collect();
    FactoryView {
        id: factory.id.clone(),
        project: factory.project.clone(),
        project_name: factory.project_name.clone(),
        source: match factory.source {
            SourceKind::Github => "github".into(),
            SourceKind::Local => "local".into(),
        },
        verification: match factory.config.verification {
            Verification::Ci { .. } => "ci".into(),
            Verification::Commands { .. } => "verify".into(),
            Verification::None => "none".into(),
        },
        closed: factory.closed,
        paused: factory.paused,
        flow,
        my_turn,
        observer_mode: factory.config.observer_mode.as_str().to_owned(),
        observer_today: if factory.observer_day == local_day(now, utc_offset_ms) as u64 {
            factory.observer_calls
        } else {
            0
        },
        observer_limit: factory.config.observer_daily_limit,
        observer_capped: factory.observer_cap_notice_day == local_day(now, utc_offset_ms) as u64
            && factory.observer_day == factory.observer_cap_notice_day,
        github_block: factory.github_block.clone(),
        follow_ups: {
            let mut open: Vec<FollowUpView> = tasks
                .values()
                .flat_map(|task| follow_ups(factory, task))
                .filter(|f| f.state == FollowUpState::Open)
                .collect();
            open.sort_by_key(|f| std::cmp::Reverse(f.at));
            open
        },
        activity: factory
            .activity
            .iter()
            .skip(
                factory
                    .activity
                    .len()
                    .saturating_sub(FACTORY_ACTIVITY_SHOWN),
            )
            .cloned()
            .collect(),
        metrics: metrics(tasks, now, utc_offset_ms),
        factory_ai: factory.config.factory_ai.clone(),
        workers: factory.config.candidates(),
        macos_notifications: factory.config.macos_notifications,
        columns,
        cancelled,
        graph,
        dependencies,
        outside_read_at: factory.outside_read_at,
        stale: factory.outside_read_failures >= 3,
        main_broken: factory.main.broken,
        auto_merge_available: factory.config.verification.configured(),
        merge_mode: match factory.config.merge_mode {
            MergeMode::Auto if factory.config.verification.configured() => "auto".into(),
            _ => "manual".into(),
        },
    }
}

fn has_run(task: &Task) -> bool {
    task.worker.is_some() || !task.attempts.is_empty() || task.last_report_at.is_some()
}

fn board_column(task: &Task) -> Option<Column> {
    match task.state {
        TaskState::Drafting => Some(Column::Before),
        TaskState::Waiting if !has_run(task) => Some(Column::Before),
        TaskState::Running | TaskState::Verifying | TaskState::Relanding | TaskState::Landed => {
            Some(Column::Moving)
        }
        TaskState::Done => Some(Column::Done),
        TaskState::Cancelled => None,
        _ => Some(Column::Stuck),
    }
}

fn board_stage(task: &Task) -> u8 {
    match task.state {
        TaskState::Drafting => 0,
        TaskState::Waiting if !has_run(task) => 0,
        TaskState::Verifying => 2,
        TaskState::Stopped
            if matches!(
                task.stop,
                Some(StopReason::VerifyFailed | StopReason::PublishRefused)
            ) =>
        {
            2
        }
        TaskState::MergeWaiting | TaskState::Landed | TaskState::Outside => 3,
        TaskState::Done => 4,
        _ => 1,
    }
}

pub fn card_view(
    factory: &Factory,
    task: &Task,
    tasks: &BTreeMap<String, Task>,
    now: UnixMs,
) -> CardView {
    let waiting = dag::waiting_for(task, tasks);
    let waiting_on: Vec<String> = waiting
        .iter()
        .map(|id| {
            tasks
                .get(id)
                .map(Task::display_id)
                .unwrap_or_else(|| id.clone())
        })
        .collect();
    let waiting_code = match task.state {
        TaskState::Waiting if !waiting_on.is_empty() => Some(WaitingFor::Predecessors),
        TaskState::Waiting if task.held.is_some() || task.held_code.is_some() => {
            Some(WaitingFor::Environment)
        }
        TaskState::Waiting => Some(WaitingFor::Slot),
        TaskState::Blocked
            if task
                .open_questions()
                .any(|question| matches!(question.kind, QuestionKind::Blocking)) =>
        {
            Some(WaitingFor::Answer)
        }
        TaskState::Blocked => Some(WaitingFor::Predecessors),
        _ => None,
    };
    let waiting_for = match waiting_code {
        Some(WaitingFor::Predecessors) if task.state == TaskState::Waiting => {
            Some(waiting_on.join(", "))
        }
        Some(WaitingFor::Predecessors) => Some("predecessor".to_owned()),
        Some(WaitingFor::Environment) => task.held.clone(),
        Some(WaitingFor::Slot) => Some("slot".to_owned()),
        Some(WaitingFor::Answer) => Some("answer".to_owned()),
        None => None,
    };
    let stop = task.stop.filter(|_| task.state == TaskState::Stopped);
    let finished_at = match task.state {
        TaskState::Done => task.done_at,
        TaskState::Cancelled | TaskState::Outside => task.cancelled_at,
        _ => None,
    };
    let folded = match task.state {
        TaskState::Done => {
            finished_at.is_some_and(|at| now.saturating_sub(at) > factory.config.done_fold_ms)
        }
        TaskState::Cancelled | TaskState::Outside => task.purged,
        _ => false,
    };
    let archived =
        finished_at.is_some_and(|at| now.saturating_sub(at) > factory.config.archive_fold_ms);
    let recovering = factory.recovering(&task.id);
    let needs_person = task.needs_person(recovering);
    CardView {
        task: task.id.clone(),
        ai_decisions: task.ai_decisions() as u32,
        permission_wait: task.permission_wait,
        recovering: recovering && task.state == TaskState::Stopped,
        display_id: task.display_id(),
        column: board_column(task).map(|column| column.as_str().to_owned()),
        title: task.card.title.clone(),
        summary: task.card.summary(),
        issue: task.issue.as_ref().map(|issue| issue.display()),
        issue_url: match (&task.issue, &factory.repo) {
            (Some(crate::model::IssueRef::Github { number }), Some(repo)) => {
                Some(format!("https://github.com/{repo}/issues/{number}"))
            }
            _ => None,
        },
        pr: task.pr.clone(),
        worker_runtime: task
            .worker
            .as_ref()
            .map(|worker| worker.runtime.as_str().to_owned()),
        worker_label: task
            .worker
            .as_ref()
            .map(|worker| worker.runtime.label().to_owned()),
        resume_at: None,
        waiting_group: (board_column(task) == Some(Column::Stuck)).then(|| {
            if needs_person || task.state == TaskState::Paused {
                "person"
            } else {
                "other"
            }
            .to_owned()
        }),
        stage: board_stage(task),
        state: task.state.as_str().to_owned(),
        state_label: stop
            .map(|reason| format!("{} ({})", task.state.label(), reason.label()))
            .unwrap_or_else(|| task.state.label().to_owned()),
        needs_person,
        waiting_on: if waiting_code == Some(WaitingFor::Predecessors) {
            waiting_on
        } else {
            Vec::new()
        },
        env_hold: task
            .held_code
            .filter(|_| waiting_code == Some(WaitingFor::Environment)),
        waiting_code,
        stop,
        pause_reason: task
            .pause_reason
            .filter(|_| task.state == TaskState::Paused),
        waiting_for,
        priority: task.human.priority,
        since: task.state_since,
        unread: task.state == TaskState::Done && !task.seen,
        folded,
        archived,
        failures: task.failures,
        external: task.card.external.clone(),
        revive_until: task
            .cancelled_at
            .filter(|_| {
                matches!(task.state, TaskState::Cancelled | TaskState::Outside) && !task.purged
            })
            .map(|at| at + factory.config.cancel_keep_ms),
        worker_pane: task
            .worker
            .as_ref()
            .filter(|_| !matches!(task.state, TaskState::Done | TaskState::Cancelled))
            .and_then(|worker| worker.pane.clone()),
    }
}

/// 결정 필요 order (D-49): blocking questions longest waiting first, then
/// other answers, then merge waiting, then stops, then to-dos.
pub fn inbox_items(
    factory: &Factory,
    tasks: &BTreeMap<String, Task>,
    now: UnixMs,
) -> Vec<InboxItem> {
    let item =
        |group: &str, rank: u8, task: Option<&Task>, text: String, since: UnixMs| InboxItem {
            group: group.to_owned(),
            kind: group.to_owned(),
            rank,
            factory: factory.id.clone(),
            task: task.map(|task| task.id.clone()),
            display_id: task.map(Task::display_id),
            title: task.map(|task| task.card.title.clone()).unwrap_or_default(),
            project: factory.project_name.clone(),
            question: None,
            text,
            stopped: None,
            holding: Holding::Progress,
            outcomes: Vec::new(),
            fallback: None,
            evidence: Vec::new(),
            resolve: None,
            command: None,
            impact: None,
            suggestion: String::new(),
            result: String::new(),
            default_action: None,
            choices: Vec::new(),
            deadline: None,
            remaining: None,
            remaining_hours: None,
            waiting_since: since,
            waiting_days: 0,
            result_code: ResultCode::Resolve,
            unblocks: Vec::new(),
            gates: Vec::new(),
            stop: None,
            env_hold: None,
            attempts: Vec::new(),
            decision_kind: None,
            observer_reason: None,
        };
    let mut items = Vec::new();
    for task in tasks.values() {
        if matches!(task.state, TaskState::Cancelled | TaskState::Outside) && task.purged {
            continue;
        }
        let base = |group: &str, rank: u8, text: String, since: UnixMs| {
            item(group, rank, Some(task), text, since)
        };
        let frees = waiting_on_this(task, tasks);
        // A request the Observer is still sorting is not a person's yet.
        for question in task.open_questions().filter(|q| q.awaits_person()) {
            let (group, rank) = match question.kind {
                QuestionKind::Blocking => ("answer", 0),
                QuestionKind::Action => ("stopped", 3),
                _ => ("answer", 1),
            };
            let mut item = base(group, rank, question.text.clone(), question.asked_at);
            item.kind = question_kind(&question.kind).to_owned();
            item.result = answer_result(task, &question.kind, tasks);
            item.result_code = answer_code(&question.kind);
            item.stop = task.stop.filter(|_| task.state == TaskState::Stopped);
            item.stopped = question.stopped.clone();
            item.holding = question_holding(task, question);
            item.outcomes = question.outcomes.clone();
            item.evidence = question.evidence.clone();
            if matches!(question.kind, QuestionKind::Blocking) {
                item.unblocks = frees.clone();
                item.waiting_days = now.saturating_sub(question.asked_at) / DAY_MS;
            }
            item.question = Some(question.id.clone());
            if let Some(routing) = &question.routing {
                item.decision_kind = routing.kind;
                item.observer_reason = routing.reason.clone();
                item.fallback = routing.fallback.clone();
            }
            item.suggestion = question.suggestion.clone();
            item.default_action = question.default_action.clone();
            item.choices = question.choices.clone();
            item.deadline = question.deadline;
            item.remaining_hours = match (&question.kind, question.deadline) {
                (QuestionKind::Blocking, _) => None,
                (_, Some(deadline)) => {
                    Some(deadline.saturating_sub(now).div_ceil(crate::model::HOUR_MS))
                }
                _ => None,
            };
            item.remaining = match (&question.kind, question.deadline) {
                (QuestionKind::Blocking, _) => {
                    let days = now.saturating_sub(question.asked_at) / DAY_MS;
                    (days >= 1).then(|| format!("{days}일째 기다림"))
                }
                (_, Some(deadline)) if deadline > now => {
                    let hours = (deadline - now).div_ceil(crate::model::HOUR_MS);
                    Some(format!("{hours}시간 남음"))
                }
                (_, Some(_)) => Some("기한 지남".into()),
                _ => None,
            };
            items.push(item);
        }
        match task.state {
            TaskState::MergeWaiting => {
                let reasons: Vec<_> = task.gates.iter().map(|gate| gate.reason()).collect();
                let mut item = base(
                    "merge",
                    2,
                    if reasons.is_empty() {
                        "머지 대기".to_owned()
                    } else {
                        format!("머지 대기: {}", reasons.join(", "))
                    },
                    task.state_since,
                );
                item.holding = Holding::Merge;
                item.choices = vec!["merge".into(), "request-changes".into(), "cancel".into()];
                item.suggestion = "merge".into();
                item.result = unblocks("머지", task, tasks);
                item.result_code = ResultCode::Merge;
                item.unblocks = frees.clone();
                item.gates = task.gates.clone();
                if let Some(report) = &task.report {
                    item.evidence.push(report.result.clone());
                }
                if let Some(pr) = &task.pr {
                    item.evidence.push(pr.url.clone());
                }
                items.push(item);
            }
            // A stop the recovery schedule still works on is not a person's
            // yet (D-44); after it, one button starts it again (B23).
            TaskState::Stopped
                if !task.open_questions().any(|question| {
                    matches!(
                        question.kind,
                        QuestionKind::Action | QuestionKind::NewTaskCap
                    )
                }) && task.needs_person(factory.recovering(&task.id)) =>
            {
                let mut item = base(
                    "stopped",
                    3,
                    task.stop_detail.clone().unwrap_or_default(),
                    task.state_since,
                );
                item.choices = vec!["retry".into()];
                item.suggestion = "retry".into();
                item.result = "같은 worktree에서 worker를 다시 시작".into();
                item.result_code = ResultCode::RestartWorker;
                item.stop = task.stop;
                item.observer_reason = task.diagnosis.clone();
                if let Some(hold) = factory.hold(&HoldKey::Task {
                    task: task.id.clone(),
                }) {
                    item.evidence.extend(hold.cause.clone());
                    item.attempts = hold.attempts.clone();
                }
                items.push(item);
            }
            // The operator closed the worker's pane: a person resumes it (D-26).
            TaskState::Paused if task.pause_reason == Some(PauseReason::PaneClosed) => {
                let mut item = base("stopped", 3, String::new(), task.state_since);
                item.kind = "paused".into();
                item.choices = vec!["resume".into(), "cancel".into()];
                item.suggestion = "resume".into();
                item.result = "같은 worktree에서 이어서 시작".into();
                item.result_code = ResultCode::ResumeWorker;
                items.push(item);
            }
            _ => {}
        }
        // Its worker's pane runs but the agent never showed a session.
        if task.start_waiting
            && matches!(
                task.state,
                TaskState::Waiting | TaskState::Running | TaskState::Relanding
            )
        {
            let mut item = base("todo", 4, String::new(), task.state_since);
            item.kind = "start".into();
            item.holding = Holding::Start;
            item.resolve = Some(format!("start:{}", task.id));
            item.result_code = ResultCode::Resolve;
            item.evidence
                .extend(task.worker.as_ref().and_then(|w| w.pane.clone()));
            items.push(item);
        }
    }
    // One sign-in to-do per Factory, whatever it stopped (B33).
    if let Some(block) = &factory.github_block {
        let mut todo = item("todo", 4, None, String::new(), block.since);
        todo.kind = "github".into();
        todo.holding = Holding::Github;
        todo.command = Some(block.command());
        todo.resolve = Some("github".into());
        todo.result_code = ResultCode::Resolve;
        todo.evidence.push(block.stage.clone());
        todo.unblocks = tasks
            .values()
            .filter(|task| task.permission_wait)
            .map(Task::display_id)
            .collect();
        items.push(todo);
    }
    for command in factory.commands.iter().filter(|c| c.resolved_at.is_none()) {
        let mut todo = item("todo", 4, None, command.cause.clone(), command.at);
        todo.kind = "command".into();
        todo.holding = Holding::Starts;
        todo.command = Some(command.command.clone());
        todo.impact = Some(command.impact.clone());
        todo.resolve = Some(command.id.clone());
        todo.result_code = ResultCode::Resolve;
        items.push(todo);
    }
    // A Factory hold the schedule could not clear (B14); a Task's is its
    // stopped item above.
    for hold in factory.holds.iter().filter(|hold| hold.escalated) {
        if matches!(hold.key, HoldKey::Task { .. }) {
            continue;
        }
        let mut todo = item(
            "todo",
            4,
            None,
            hold.cause.clone().unwrap_or_default(),
            hold.since,
        );
        todo.kind = "hold".into();
        todo.holding = match hold.key {
            HoldKey::Reads => Holding::Github,
            _ => Holding::Starts,
        };
        todo.env_hold = match hold.key {
            HoldKey::Start { hold } => Some(hold),
            _ => None,
        };
        todo.attempts = hold.attempts.clone();
        todo.resolve = Some(format!("hold:{}", hold_name(&hold.key)));
        todo.result_code = ResultCode::Resolve;
        items.push(todo);
    }
    items
}

/// A hold's name in a to-do's item, as `hide factory resolve` takes it.
pub fn hold_name(key: &HoldKey) -> String {
    match key {
        HoldKey::Start { hold } => format!(
            "start-{}",
            match hold {
                EnvHold::DiskFloor => "disk_floor",
                EnvHold::DiskFull => "disk_full",
                EnvHold::MemoryCritical => "memory_critical",
            }
        ),
        HoldKey::Halt => "halt".into(),
        HoldKey::Reads => "reads".into(),
        HoldKey::Task { task } => format!("task-{task}"),
    }
}

/// What an open question holds up when its asker did not say.
fn question_holding(task: &Task, question: &Question) -> Holding {
    match question.kind {
        QuestionKind::Blocking => Holding::Worker,
        QuestionKind::Default => Holding::Continues,
        QuestionKind::ScopeChange { .. } if question.default_action.is_some() => Holding::Continues,
        QuestionKind::Intake
        | QuestionKind::ConfirmCard
        | QuestionKind::Split { .. }
        | QuestionKind::ProposedTask { .. } => Holding::Start,
        _ if task.state == TaskState::MergeWaiting => Holding::Merge,
        _ => Holding::Progress,
    }
}

/// A Task's follow-up candidates, in the order they were found.
pub fn follow_ups(factory: &Factory, task: &Task) -> Vec<FollowUpView> {
    task.discoveries
        .iter()
        .filter_map(|discovery| {
            let follow_up = discovery.follow_up.as_ref()?;
            Some(FollowUpView {
                task: task.id.clone(),
                display_id: task.display_id(),
                discovery: discovery.id.clone(),
                text: discovery.text.clone(),
                state: follow_up.state,
                issue: follow_up.issue.as_ref().map(|issue| issue.display()),
                issue_url: follow_up
                    .issue
                    .as_ref()
                    .and_then(|issue| issue_url(factory, issue)),
                became: follow_up.task.clone(),
                failure: follow_up.failure.clone(),
                at: follow_up.at,
            })
        })
        .collect()
}

fn issue_url(factory: &Factory, issue: &crate::model::IssueRef) -> Option<String> {
    match (issue, &factory.repo) {
        (crate::model::IssueRef::Github { number }, Some(repo)) => {
            Some(format!("https://github.com/{repo}/issues/{number}"))
        }
        _ => None,
    }
}

/// The three numbers over the last seven local days (D-45).
fn metrics(tasks: &BTreeMap<String, Task>, now: UnixMs, utc_offset_ms: i64) -> Metrics {
    let since = local_day(now, utc_offset_ms) - (METRICS_DAYS - 1);
    let within = |at: UnixMs| local_day(at, utc_offset_ms) >= since && at <= now;
    let mut metrics = Metrics::default();
    let mut items = 0u32;
    let mut waits: Vec<u64> = Vec::new();
    for task in tasks.values() {
        if task.state == TaskState::Done && task.done_at.is_some_and(within) {
            metrics.finished += 1;
            items += task.person_items;
        }
        if let Some(started) = task.first_started_at.filter(|at| within(*at)) {
            metrics.started += 1;
            waits.push(started.saturating_sub(task.created_at));
        }
        for decision in &task.decisions {
            // Only Factory AI's decisions can be changed, so a changed one
            // was Factory AI's.
            let ai = decision.by == OBSERVER || decision.changed.is_some();
            if ai && within(decision.at) {
                metrics.ai_decisions += 1;
            }
            if decision
                .changed
                .as_ref()
                .is_some_and(|change| within(change.at))
            {
                metrics.overridden += 1;
            }
        }
    }
    metrics.person_items_tenths = (items * 10).checked_div(metrics.finished);
    if !waits.is_empty() {
        waits.sort_unstable();
        let middle = waits.len() / 2;
        metrics.start_median_ms = Some(if waits.len().is_multiple_of(2) {
            (waits[middle - 1] + waits[middle]) / 2
        } else {
            waits[middle]
        });
    }
    metrics.override_percent = (metrics.overridden * 100).checked_div(metrics.ai_decisions);
    metrics
}

fn question_kind(kind: &QuestionKind) -> &'static str {
    match kind {
        QuestionKind::Intake => "intake",
        QuestionKind::Split { .. } => "split",
        QuestionKind::Default => "default",
        QuestionKind::Blocking => "blocking",
        QuestionKind::ScopeChange { .. } => "scope_change",
        QuestionKind::NewTaskCap => "new_task_cap",
        QuestionKind::ProposedTask { .. } => "proposed_task",
        QuestionKind::Action => "action",
        QuestionKind::ConfirmCard => "confirm_card",
    }
}

/// The result line next to the send button (stage 2 B9).
fn answer_result(task: &Task, kind: &QuestionKind, tasks: &BTreeMap<String, Task>) -> String {
    match kind {
        QuestionKind::Blocking => unblocks("worker를 깨워 이어감", task, tasks),
        QuestionKind::Default | QuestionKind::ScopeChange { .. } => {
            "기본 행동과 다르면 worker가 반영, 같으면 머지로 감".into()
        }
        QuestionKind::Intake | QuestionKind::ConfirmCard => "남은 질문이 없으면 Ready".into(),
        QuestionKind::Split { pieces } => format!("{}개 Task로 나눔", pieces.len()),
        QuestionKind::ProposedTask { .. } => "승인하면 정리 중으로 들어감".into(),
        QuestionKind::NewTaskCap => "고른 대로 쪼개기, 계속, 멈춤".into(),
        QuestionKind::Action => "고른 행동을 실행".into(),
    }
}

fn answer_code(kind: &QuestionKind) -> ResultCode {
    match kind {
        QuestionKind::Blocking => ResultCode::WakeWorker,
        QuestionKind::Default | QuestionKind::ScopeChange { .. } => ResultCode::ApplyOrMerge,
        QuestionKind::Intake | QuestionKind::ConfirmCard => ResultCode::Ready,
        QuestionKind::Split { .. } => ResultCode::Split,
        QuestionKind::ProposedTask { .. } => ResultCode::Drafting,
        QuestionKind::NewTaskCap => ResultCode::NewTaskCapChoice,
        QuestionKind::Action => ResultCode::RunAction,
    }
}

/// The waiting Tasks that depend on `task`, by display id.
fn waiting_on_this(task: &Task, tasks: &BTreeMap<String, Task>) -> Vec<String> {
    tasks
        .values()
        .filter(|other| other.card.depends_on.contains(&task.id))
        .filter(|other| matches!(other.state, TaskState::Waiting | TaskState::Blocked))
        .map(Task::display_id)
        .collect()
}

/// `"<what> , 끝나면 #421이 풀림"` when a waiting Task depends on this one.
fn unblocks(what: &str, task: &Task, tasks: &BTreeMap<String, Task>) -> String {
    let waiting = waiting_on_this(task, tasks);
    if waiting.is_empty() {
        what.to_owned()
    } else {
        format!("{what}, 끝나면 {}이 풀림", waiting.join(", "))
    }
}

/// The Task page (stage 2 B18, B19): what `show` answers. Like the summary,
/// a contract with stage 2: add fields, never rename or remove.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDetail {
    pub card: CardView,
    pub factory: String,
    pub project: String,
    pub goal: String,
    pub criteria: Vec<String>,
    pub out_of_scope: Vec<String>,
    /// The small chain: predecessors, then the Tasks waiting on this one.
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub attachments: Vec<Attachment>,
    pub pr: Option<PullRequest>,
    /// `n/3`, or `검증 없음` for a Factory without verification.
    pub verification: String,
    pub attempts: Vec<AttemptView>,
    /// Every decision with its id, who made it, and whether a person may
    /// still change it (D-35, B28).
    pub decisions: Vec<DecisionView>,
    pub questions: Vec<Question>,
    pub discoveries: Vec<Discovery>,
    /// Each completion criterion with what the last check said (B10).
    pub checklist: Vec<CriterionView>,
    /// The worker's last report in four parts (B30).
    pub report: Option<WorkerReport>,
    /// The Task's activity, oldest first (B30).
    pub activity: Vec<Activity>,
    /// Its follow-up candidates (B27).
    pub follow_ups: Vec<FollowUpView>,
    /// The issue as written, for the folded original (B27); none without
    /// an issue.
    pub issue_text: Option<String>,
    /// Why merge waits for a person (D-25).
    pub gates: Vec<String>,
    /// The same as codes.
    pub gate_codes: Vec<Gate>,
    /// The actions this state allows (Q29).
    pub allowed: Vec<String>,
    pub stop: Option<String>,
    /// `stop` as a code.
    pub stop_code: Option<StopReason>,
    pub merge_sha: Option<String>,
    pub worker_name: Option<String>,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    /// The worker that actually started, and why its candidate (B33).
    pub worker: Option<WorkerLine>,
    /// The Observer's one line under a no-report stop (B23).
    pub diagnosis: Option<String>,
    /// Automatic restarts used since a person last started it (D-25).
    #[serde(default)]
    pub auto_restarts: u32,
    /// When the worker's current rest began, as the core saw it (B43).
    pub resting_since: Option<UnixMs>,
    /// The candidate a person pinned, 1 first.
    pub pinned_worker: Option<usize>,
    /// The candidate the review picked, 1 first, and why (D-41).
    pub ai_picked_worker: Option<usize>,
    pub ai_pick_reason: Option<String>,
    /// When the engine woke the resting worker and asked for a diagnosis.
    pub woke_at: Option<UnixMs>,
    pub diagnosed_at: Option<UnixMs>,
    /// The worker text that diagnosis read (D-37).
    pub diagnosed_from: Option<WorkerTextSource>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionView {
    /// `R<n>`, what `hide factory answer --decision` takes.
    pub id: String,
    pub text: String,
    pub by: DecisionBy,
    /// Who recorded it, as stored.
    pub recorded_by: String,
    pub source: Option<DecisionSource>,
    pub kind: Option<DecisionKind>,
    pub reason: Option<String>,
    pub at: UnixMs,
    /// Factory AI's, and the Task is not finished.
    pub overridable: bool,
    /// What Factory AI had decided, when a person changed it.
    pub changed: Option<DecisionChange>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriterionView {
    pub text: String,
    /// None until a check judged it.
    pub state: Option<CriterionState>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerLine {
    pub agent: String,
    pub label: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The candidate's description when the review picked it, with why.
    pub picked: Option<String>,
    pub pick_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptView {
    /// Its place in the Task's attempts, from 1.
    pub number: u32,
    pub stage: String,
    pub started_at: UnixMs,
    /// `passed`, `failed`, `environment`, `cancelled` or `running`.
    pub outcome: String,
    pub check: Option<String>,
    /// The CI run link or the log path.
    pub link: Option<String>,
    /// The end of the run's log.
    pub log_tail: Option<String>,
}

const LOG_TAIL: usize = 4 * 1024;

pub fn detail(
    factory: &Factory,
    task: &Task,
    tasks: &BTreeMap<String, Task>,
    allowed: Vec<&str>,
    now: UnixMs,
    log_tail: &mut dyn FnMut(&str) -> Option<String>,
) -> TaskDetail {
    let name = |id: &String| {
        tasks
            .get(id)
            .map(Task::display_id)
            .unwrap_or_else(|| id.clone())
    };
    let after = tasks
        .values()
        .filter(|other| other.card.depends_on.contains(&task.id))
        .map(Task::display_id)
        .collect();
    let last = task.attempts.len().saturating_sub(1);
    let attempts = task
        .attempts
        .iter()
        .enumerate()
        .map(|(at, attempt)| {
            let (outcome, check, link) = match &attempt.outcome {
                // Only the last attempt can be in flight; an earlier one
                // without an answer was ended by a store from before
                // cancelled runs were closed.
                None if at < last => ("cancelled", None, attempt.log.clone()),
                None => ("running", None, attempt.log.clone()),
                Some(AttemptOutcome::Cancelled) => ("cancelled", None, attempt.log.clone()),
                Some(AttemptOutcome::Passed) => ("passed", None, attempt.log.clone()),
                Some(AttemptOutcome::Failed { check, link }) => (
                    "failed",
                    Some(check.clone()),
                    Some(link.clone())
                        .filter(|l| !l.is_empty())
                        .or_else(|| attempt.log.clone()),
                ),
                Some(AttemptOutcome::Environment { check, .. }) => {
                    ("environment", Some(check.clone()), attempt.log.clone())
                }
            };
            AttemptView {
                // The place in the list, which a store from before attempts
                // were numbered that way also reads correctly by.
                number: at as u32 + 1,
                stage: match attempt.stage {
                    AttemptStage::Task => "task".into(),
                    AttemptStage::PreMerge => "pre_merge".into(),
                },
                started_at: attempt.started_at,
                outcome: outcome.into(),
                check,
                log_tail: attempt
                    .log
                    .as_deref()
                    .and_then(&mut *log_tail)
                    .map(|tail| last_bytes(&tail, LOG_TAIL)),
                link,
            }
        })
        .collect();
    TaskDetail {
        card: card_view(factory, task, tasks, now),
        factory: factory.id.clone(),
        project: factory.project.clone(),
        goal: task.card.goal.clone(),
        criteria: task.card.criteria.clone(),
        out_of_scope: task.card.out_of_scope.clone(),
        before: task.card.depends_on.iter().map(name).collect(),
        after,
        attachments: task.attachments.clone(),
        pr: task.pr.clone(),
        verification: if factory.config.verification.configured() {
            format!("{}/{}", task.failures, factory.config.verify_failure_limit)
        } else {
            "검증 없음".into()
        },
        attempts,
        decisions: decision_views(task),
        questions: task.questions.clone(),
        discoveries: task.discoveries.clone(),
        checklist: checklist(&task.card.criteria, &task.criteria_check),
        report: task.report.clone(),
        activity: task.activity.clone(),
        follow_ups: follow_ups(factory, task),
        issue_text: task.issue.as_ref().map(|_| task.card.goal.clone()),
        gates: task
            .gates
            .iter()
            .map(|gate| gate.reason().to_owned())
            .collect(),
        gate_codes: task.gates.clone(),
        allowed: allowed.into_iter().map(str::to_owned).collect(),
        stop: task.stop.map(|reason| reason.label().to_owned()),
        stop_code: task.stop,
        merge_sha: task.merge_sha.clone(),
        worker_name: task.worker.as_ref().map(|worker| worker.name.clone()),
        worktree: task.worker.as_ref().map(|worker| worker.worktree.clone()),
        branch: task.worker.as_ref().map(|worker| worker.branch.clone()),
        worker: task.worker.as_ref().map(|worker| {
            let candidates = factory.config.candidates();
            let pick = task
                .ai_pick
                .as_ref()
                .filter(|_| task.human.worker.is_none() && task.human.runtime.is_none())
                .and_then(|pick| candidates.get(pick.index).map(|c| (c, pick)));
            WorkerLine {
                agent: worker.runtime.as_str().to_owned(),
                label: worker.runtime.label().to_owned(),
                model: worker.model.clone(),
                effort: worker.effort.clone(),
                picked: pick.map(|(candidate, _)| candidate.description.clone()),
                pick_reason: pick.map(|(_, pick)| pick.reason.clone()),
            }
        }),
        diagnosis: task.diagnosis.clone(),
        auto_restarts: task.auto_restarts,
        resting_since: task.rest_seen.filter(|_| task.state == TaskState::Running),
        pinned_worker: task.human.worker.map(|index| index + 1),
        ai_picked_worker: task.ai_pick.as_ref().map(|pick| pick.index + 1),
        ai_pick_reason: task.ai_pick.as_ref().map(|pick| pick.reason.clone()),
        woke_at: task.recovery.as_ref().and_then(|r| r.woke_at),
        diagnosed_at: task.recovery.as_ref().and_then(|r| r.diagnosed_at),
        diagnosed_from: task.recovery.as_ref().and_then(|r| r.diagnosed_from),
    }
}

fn decision_views(task: &Task) -> Vec<DecisionView> {
    let finished = matches!(
        task.state,
        TaskState::Done | TaskState::Landed | TaskState::Cancelled | TaskState::Outside
    );
    task.decisions
        .iter()
        .enumerate()
        .map(|(index, record)| DecisionView {
            id: decision_id(index),
            text: record.text.clone(),
            by: if record.by == OBSERVER {
                DecisionBy::Ai
            } else if record.by.starts_with("worker:") {
                DecisionBy::Worker
            } else {
                DecisionBy::Person
            },
            recorded_by: record.by.clone(),
            source: record.source,
            kind: record.kind,
            reason: record.reason.clone(),
            at: record.at,
            overridable: task.decision_changeable(record) && !finished,
            changed: record.changed.clone(),
        })
        .collect()
}

/// The card's criteria with the check's verdicts, matched by text and, when
/// the check named them differently but counted the same, by place.
fn checklist(criteria: &[String], verdicts: &[CriterionVerdict]) -> Vec<CriterionView> {
    criteria
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let verdict = verdicts
                .iter()
                .find(|v| v.criterion.trim() == text.trim())
                .or_else(|| (verdicts.len() == criteria.len()).then(|| &verdicts[index]));
            CriterionView {
                text: text.clone(),
                state: verdict.map(|v| v.state),
                reason: verdict.map(|v| v.reason.clone()).filter(|r| !r.is_empty()),
            }
        })
        .collect()
}

/// The last `limit` bytes of `text`, from a character boundary.
fn last_bytes(text: &str, limit: usize) -> String {
    let mut start = text.len().saturating_sub(limit);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
}

#[cfg(test)]
mod tests {
    //! The codes stage 2 decodes are a contract. Each list is matched
    //! exhaustively, so a new variant fails to compile here until it is
    //! listed, and then fails until its wire value is pinned below and in
    //! `docs/factory.md`.
    use super::*;
    use crate::model::{DecisionSource, ObserverMode};

    fn wire<T: Serialize>(values: &[T]) -> Vec<String> {
        values
            .iter()
            .map(|value| match serde_json::to_value(value).unwrap() {
                serde_json::Value::String(text) => text,
                other => panic!("a code serializes as a string, got {other}"),
            })
            .collect()
    }

    fn complete<T: Copy>(all: &[T], index: fn(T) -> usize) {
        let indexes: Vec<usize> = all.iter().map(|value| index(*value)).collect();
        assert_eq!(
            indexes,
            (0..all.len()).collect::<Vec<_>>(),
            "ALL lists every variant once, in order"
        );
    }

    #[test]
    fn a_task_done_before_nine_in_seoul_is_done_today_there() {
        const HOUR: i64 = 3_600_000;
        // 2026-10-07 05:00 UTC is 14:00 in Seoul; 2026-10-06 23:30 UTC is 08:30 there.
        let now = 1_791_349_200_000;
        let done = now - 5 * HOUR as u64 - 30 * 60_000;
        assert_eq!(local_day(done, 9 * HOUR), local_day(now, 9 * HOUR));
        assert_ne!(local_day(done, 0), local_day(now, 0), "a different UTC day");
        // Seoul's day ends at 15:00 UTC.
        assert_ne!(
            local_day(now + 10 * HOUR as u64, 9 * HOUR),
            local_day(now, 9 * HOUR)
        );
    }

    #[test]
    fn every_summary_code_is_pinned() {
        complete(&Gate::ALL, |gate| match gate {
            Gate::ReviewDirectly => 0,
            Gate::ApprovedScopeChange => 1,
            Gate::BreakingChange => 2,
            Gate::NoVerification => 3,
            Gate::RiskPath => 4,
            Gate::ManualMode => 5,
            Gate::OpenQuestion => 6,
            Gate::CheckFailed => 7,
            Gate::AutonomyDiff => 8,
            Gate::DirtyMain => 9,
            Gate::MergeRefused => 10,
        });
        assert_eq!(
            wire(&Gate::ALL),
            [
                "review_directly",
                "approved_scope_change",
                "breaking_change",
                "no_verification",
                "risk_path",
                "manual_mode",
                "open_question",
                "check_failed",
                "autonomy_diff",
                "dirty_main",
                "merge_refused",
            ]
        );
        complete(&StopReason::ALL, |reason| match reason {
            StopReason::NoReport => 0,
            StopReason::Stalled => 1,
            StopReason::VerifyFailed => 2,
            StopReason::NewTaskCap => 3,
            StopReason::EnvironmentRepeated => 4,
            StopReason::WorkerStart => 5,
            StopReason::PublishRefused => 6,
            StopReason::WorkerGone => 7,
        });
        assert_eq!(
            wire(&StopReason::ALL),
            [
                "no_report",
                "stalled",
                "verify_failed",
                "new_task_cap",
                "environment_repeated",
                "worker_start",
                "publish_refused",
                "worker_gone",
            ]
        );
        complete(&EnvHold::ALL, |hold| match hold {
            EnvHold::DiskFloor => 0,
            EnvHold::DiskFull => 1,
            EnvHold::MemoryCritical => 2,
        });
        assert_eq!(
            wire(&EnvHold::ALL),
            ["disk_floor", "disk_full", "memory_critical"]
        );
        complete(&WaitingFor::ALL, |waiting| match waiting {
            WaitingFor::Predecessors => 0,
            WaitingFor::Slot => 1,
            WaitingFor::Environment => 2,
            WaitingFor::Answer => 3,
        });
        assert_eq!(
            wire(&WaitingFor::ALL),
            ["predecessors", "slot", "environment", "answer"]
        );
        complete(&ResultCode::ALL, |code| match code {
            ResultCode::WakeWorker => 0,
            ResultCode::ApplyOrMerge => 1,
            ResultCode::Ready => 2,
            ResultCode::Split => 3,
            ResultCode::Drafting => 4,
            ResultCode::NewTaskCapChoice => 5,
            ResultCode::RunAction => 6,
            ResultCode::Merge => 7,
            ResultCode::RestartWorker => 8,
            ResultCode::ResumeWorker => 9,
            ResultCode::Resolve => 10,
        });
        assert_eq!(
            wire(&ResultCode::ALL),
            [
                "wake_worker",
                "apply_or_merge",
                "ready",
                "split",
                "drafting",
                "new_task_cap_choice",
                "run_action",
                "merge",
                "restart_worker",
                "resume_worker",
                "resolve",
            ]
        );
        complete(&Holding::ALL, |holding| match holding {
            Holding::Worker => 0,
            Holding::Start => 1,
            Holding::Merge => 2,
            Holding::Progress => 3,
            Holding::Starts => 4,
            Holding::Github => 5,
            Holding::Continues => 6,
        });
        assert_eq!(
            wire(&Holding::ALL),
            [
                "worker",
                "start",
                "merge",
                "progress",
                "starts",
                "github",
                "continues"
            ]
        );
        complete(&DecisionBy::ALL, |by| match by {
            DecisionBy::Person => 0,
            DecisionBy::Ai => 1,
            DecisionBy::Worker => 2,
        });
        assert_eq!(wire(&DecisionBy::ALL), ["person", "ai", "worker"]);
        complete(&DecisionSource::ALL, |source| match source {
            DecisionSource::Answer => 0,
            DecisionSource::Assumption => 1,
            DecisionSource::SendBack => 2,
            DecisionSource::Worker => 3,
            DecisionSource::RequestChanges => 4,
            DecisionSource::RiskMerge => 5,
        });
        assert_eq!(
            wire(&DecisionSource::ALL),
            [
                "answer",
                "assumption",
                "send_back",
                "worker",
                "request_changes",
                "risk_merge"
            ]
        );
        complete(&FollowUpState::ALL, |state| match state {
            FollowUpState::Open => 0,
            FollowUpState::Issue => 1,
            FollowUpState::Factory => 2,
            FollowUpState::Discarded => 3,
        });
        assert_eq!(
            wire(&FollowUpState::ALL),
            ["open", "issue", "factory", "discarded"]
        );
        complete(&CriterionState::ALL, |state| match state {
            CriterionState::Met => 0,
            CriterionState::Unmet => 1,
            CriterionState::Unknown => 2,
        });
        assert_eq!(wire(&CriterionState::ALL), ["met", "unmet", "unknown"]);
        complete(
            &crate::model::RecoveryOutcome::ALL,
            |outcome| match outcome {
                crate::model::RecoveryOutcome::Improved => 0,
                crate::model::RecoveryOutcome::Partial => 1,
                crate::model::RecoveryOutcome::Unchanged => 2,
            },
        );
        assert_eq!(
            wire(&crate::model::RecoveryOutcome::ALL),
            ["improved", "partial", "unchanged"]
        );
        complete(&DecisionKind::ALL, |kind| match kind {
            DecisionKind::A => 0,
            DecisionKind::B => 1,
            DecisionKind::C => 2,
            DecisionKind::D => 3,
            DecisionKind::E => 4,
        });
        assert_eq!(wire(&DecisionKind::ALL), ["A", "B", "C", "D", "E"]);
        complete(&PauseReason::ALL, |reason| match reason {
            PauseReason::Person => 0,
            PauseReason::PaneClosed => 1,
        });
        assert_eq!(wire(&PauseReason::ALL), ["person", "pane_closed"]);
        complete(&ObserverMode::ALL, |mode| match mode {
            ObserverMode::Manual => 0,
            ObserverMode::Assist => 1,
            ObserverMode::Autonomous => 2,
        });
        assert_eq!(wire(&ObserverMode::ALL), ["manual", "assist", "autonomous"]);
        complete(&WorkerTextSource::ALL, |source| match source {
            WorkerTextSource::UserTurn => 0,
            WorkerTextSource::LastAnswer => 1,
            WorkerTextSource::Screen => 2,
        });
        assert_eq!(
            wire(&WorkerTextSource::ALL),
            ["user_turn", "last_answer", "screen"]
        );
    }
}
