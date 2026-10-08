//! `FactorySummary`: the one value the CLI's `status` and `inbox` print and
//! the stage 2 screens draw (D-50). Every number in it is derived here from
//! the store's Tasks (design #4, #10); the shell computes nothing.
//!
//! Board columns follow movement; engine lifecycle states remain unchanged.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::dag;
use crate::judgment::WorkerTextSource;
use crate::model::{
    Attachment, AttemptOutcome, AttemptStage, Column, DAY_MS, DecisionKind, DecisionRecord,
    Discovery, EnvHold, Factory, FactoryAi, Gate, MergeMode, NoticeCode, PauseReason, PullRequest,
    Question, QuestionKind, Runtime, SourceKind, StopReason, Task, TaskState, UnixMs, Verification,
    WorkerCandidate,
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
    Acknowledge,
    Merge,
    /// The worker restarts in the same worktree.
    RestartWorker,
    /// A paused Task resumes in the same worktree.
    ResumeWorker,
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
        Self::Acknowledge,
        Self::Merge,
        Self::RestartWorker,
        Self::ResumeWorker,
    ];
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactorySummary {
    /// The one person-facing number: answers, merge waits and stops across
    /// Factories; notices are not counted (D-43).
    pub my_turn: u32,
    /// Notices across Factories, shown below the line (D-43).
    #[serde(default)]
    pub notices: u32,
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
    #[serde(default)]
    pub notices: u32,
    /// `manual`, `assist` or `autonomous` (직접, 함께, 맡김).
    #[serde(default)]
    pub observer_mode: String,
    /// Observer calls today and the daily cap (D-34).
    #[serde(default)]
    pub observer_today: u32,
    #[serde(default)]
    pub observer_limit: u32,
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

/// One person-facing item (D-19, D-49 order).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxItem {
    pub group: String,
    /// The question kind, `merge` or `stopped`.
    pub kind: String,
    pub rank: u8,
    pub factory: String,
    pub task: String,
    pub display_id: String,
    pub title: String,
    pub project: String,
    pub question: Option<String>,
    /// Why it is the person's turn, in the engine's words (stage 2 B9).
    pub text: String,
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
    /// What a notice says, as a code.
    pub notice: Option<NoticeCode>,
    /// The decision a notice is about; "다른 답" answers that question.
    pub refers_to: Option<String>,
    /// The Observer's kind and reason for a request it sorted.
    pub decision_kind: Option<DecisionKind>,
    pub observer_reason: Option<String>,
    /// Whether the decision can still be changed (its Task is not finished).
    #[serde(default)]
    pub overridable: bool,
}

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
        let notices = items.iter().filter(|item| item.group == "notice").count() as u32;
        let view = factory_view(
            factory,
            &mine,
            (items.len() as u32 - notices, notices),
            now,
            utc_offset_ms,
            runtime_holds,
        );
        summary.my_turn += view.my_turn;
        summary.notices += view.notices;
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
    (my_turn, notices): (u32, u32),
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
        notices,
        observer_mode: factory.config.observer_mode.as_str().to_owned(),
        observer_today: if factory.observer_day == local_day(now, utc_offset_ms) as u64 {
            factory.observer_calls
        } else {
            0
        },
        observer_limit: factory.config.observer_daily_limit,
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
    CardView {
        task: task.id.clone(),
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
            if task.needs_person() || task.state == TaskState::Paused {
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
        needs_person: task.needs_person(),
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

/// Inbox order (D-49): blocking questions longest waiting first, then other
/// answers, then merge waiting, then stopped, then notices.
pub fn inbox_items(
    factory: &Factory,
    tasks: &BTreeMap<String, Task>,
    now: UnixMs,
) -> Vec<InboxItem> {
    let mut items = Vec::new();
    for task in tasks.values() {
        if matches!(task.state, TaskState::Cancelled | TaskState::Outside) && task.purged {
            continue;
        }
        let base = |group: &str, rank: u8, text: String, since: UnixMs| InboxItem {
            group: group.to_owned(),
            kind: group.to_owned(),
            rank,
            factory: factory.id.clone(),
            task: task.id.clone(),
            display_id: task.display_id(),
            title: task.card.title.clone(),
            project: factory.project_name.clone(),
            question: None,
            text,
            suggestion: String::new(),
            result: String::new(),
            default_action: None,
            choices: Vec::new(),
            deadline: None,
            remaining: None,
            remaining_hours: None,
            waiting_since: since,
            waiting_days: 0,
            result_code: ResultCode::Acknowledge,
            unblocks: Vec::new(),
            gates: Vec::new(),
            stop: None,
            notice: None,
            refers_to: None,
            decision_kind: None,
            observer_reason: None,
            overridable: false,
        };
        let frees = waiting_on_this(task, tasks);
        let finished = matches!(
            task.state,
            TaskState::Done | TaskState::Landed | TaskState::Cancelled | TaskState::Outside
        );
        // A request the Observer is still sorting is not a person's yet.
        for question in task.open_questions().filter(|q| q.awaits_person()) {
            let (group, rank) = match question.kind {
                QuestionKind::Blocking => ("answer", 0),
                QuestionKind::Notice => ("notice", 4),
                QuestionKind::Action | QuestionKind::Proposal { .. } => ("stopped", 3),
                _ => ("answer", 1),
            };
            let mut item = base(group, rank, question.text.clone(), question.asked_at);
            item.kind = question_kind(&question.kind).to_owned();
            item.result = answer_result(task, &question.kind, tasks);
            item.result_code = answer_code(&question.kind);
            item.stop = task.stop.filter(|_| task.state == TaskState::Stopped);
            if matches!(question.kind, QuestionKind::Blocking) {
                item.unblocks = frees.clone();
            }
            if matches!(question.kind, QuestionKind::Blocking) {
                item.waiting_days = now.saturating_sub(question.asked_at) / DAY_MS;
            }
            item.question = Some(question.id.clone());
            item.notice = question.notice;
            item.refers_to = question.refers_to.clone();
            item.overridable = question.refers_to.is_some()
                && !finished
                && task
                    .questions
                    .iter()
                    .find(|q| Some(&q.id) == question.refers_to.as_ref())
                    .is_some_and(|q| {
                        q.answer
                            .as_ref()
                            .is_some_and(|a| a.relayed_by == crate::model::OBSERVER)
                    });
            // A notice says the kind of the decision it is about.
            let routing = question.routing.as_ref().or_else(|| {
                task.questions
                    .iter()
                    .find(|q| Some(&q.id) == question.refers_to.as_ref())
                    .and_then(|q| q.routing.as_ref())
            });
            if let Some(routing) = routing {
                item.decision_kind = routing.kind;
                item.observer_reason = routing.reason.clone();
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
                item.choices = vec!["merge".into(), "request-changes".into(), "cancel".into()];
                item.suggestion = "merge".into();
                item.result = unblocks("머지", task, tasks);
                item.result_code = ResultCode::Merge;
                item.unblocks = frees.clone();
                item.gates = task.gates.clone();
                items.push(item);
            }
            TaskState::Stopped
                if !task.open_questions().any(|question| {
                    matches!(
                        question.kind,
                        QuestionKind::Action | QuestionKind::NewTaskCap
                    )
                }) =>
            {
                let mut item = base(
                    "stopped",
                    3,
                    match &task.stop_detail {
                        Some(detail) => format!(
                            "멈춤: {} - {detail}",
                            task.stop.map(|reason| reason.label()).unwrap_or("?")
                        ),
                        None => format!(
                            "멈춤: {}",
                            task.stop.map(|reason| reason.label()).unwrap_or("?")
                        ),
                    },
                    task.state_since,
                );
                item.choices = vec!["retry".into(), "cancel".into()];
                item.suggestion = "retry".into();
                item.result = "같은 worktree에서 worker를 다시 시작".into();
                item.result_code = ResultCode::RestartWorker;
                item.stop = task.stop;
                item.observer_reason = task.diagnosis.clone();
                items.push(item);
            }
            // The operator closed the worker's pane: a person resumes it (D-26).
            TaskState::Paused if task.pause_reason == Some(PauseReason::PaneClosed) => {
                let mut item = base(
                    "stopped",
                    3,
                    "일시정지: 작업자 pane을 닫음".to_owned(),
                    task.state_since,
                );
                item.kind = "paused".into();
                item.choices = vec!["resume".into(), "cancel".into()];
                item.suggestion = "resume".into();
                item.result = "같은 worktree에서 이어서 시작".into();
                item.result_code = ResultCode::ResumeWorker;
                items.push(item);
            }
            _ => {}
        }
    }
    items
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
        QuestionKind::Proposal { .. } => "proposal",
        QuestionKind::Notice => "notice",
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
        QuestionKind::Action | QuestionKind::Proposal { .. } => "고른 행동을 실행".into(),
        QuestionKind::Notice => "확인".into(),
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
        QuestionKind::Action | QuestionKind::Proposal { .. } => ResultCode::RunAction,
        QuestionKind::Notice => ResultCode::Acknowledge,
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
    pub decisions: Vec<DecisionRecord>,
    pub questions: Vec<Question>,
    pub discoveries: Vec<Discovery>,
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
    pub number: u32,
    pub stage: String,
    pub started_at: UnixMs,
    /// `passed`, `failed`, `environment` or `running`.
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
    let attempts = task
        .attempts
        .iter()
        .map(|attempt| {
            let (outcome, check, link) = match &attempt.outcome {
                None => ("running", None, attempt.log.clone()),
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
                number: attempt.number,
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
        decisions: task.decisions.clone(),
        questions: task.questions.clone(),
        discoveries: task.discoveries.clone(),
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
    use crate::model::ObserverMode;

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
            ResultCode::Acknowledge => 7,
            ResultCode::Merge => 8,
            ResultCode::RestartWorker => 9,
            ResultCode::ResumeWorker => 10,
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
                "acknowledge",
                "merge",
                "restart_worker",
                "resume_worker",
            ]
        );
        complete(&NoticeCode::ALL, |code| match code {
            NoticeCode::AiAnswered => 0,
            NoticeCode::AiCardFixed => 1,
            NoticeCode::AiNewTask => 2,
            NoticeCode::AiRiskMerge => 3,
            NoticeCode::DailyLimit => 4,
        });
        assert_eq!(
            wire(&NoticeCode::ALL),
            [
                "ai_answered",
                "ai_card_fixed",
                "ai_new_task",
                "ai_risk_merge",
                "daily_limit",
            ]
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
