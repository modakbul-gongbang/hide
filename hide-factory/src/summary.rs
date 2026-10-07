//! `FactorySummary`: the one value the CLI's `status` and `inbox` print and
//! the stage 2 screens draw (D-50). Every number in it is derived here from
//! the store's Tasks (design #4, #10); the shell computes nothing.
//!
//! This type is a contract with stage 2: add fields, never rename or remove.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::dag;
use crate::model::{
    Attachment, AttemptOutcome, AttemptStage, Column, DAY_MS, DecisionRecord, Discovery, EnvHold,
    Factory, Gate, MergeMode, PullRequest, Question, QuestionKind, SourceKind, StopReason, Task,
    TaskState, UnixMs, Verification,
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
}

impl ResultCode {
    pub const ALL: [Self; 10] = [
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
    ];
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactorySummary {
    /// The one person-facing number: open inbox items across Factories (D-37).
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
    pub flow: Flow,
    pub my_turn: u32,
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
    pub drafting: u32,
    pub waiting: u32,
    pub running: u32,
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
}

pub fn build(factories: &[&Factory], tasks: &[&Task], now: UnixMs) -> FactorySummary {
    let mut summary = FactorySummary::default();
    for factory in factories {
        let mine: BTreeMap<String, Task> = tasks
            .iter()
            .filter(|task| task.factory == factory.id)
            .map(|task| (task.id.clone(), (*task).clone()))
            .collect();
        let items = inbox_items(factory, &mine, now);
        let view = factory_view(factory, &mine, items.len() as u32, now);
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

fn factory_view(
    factory: &Factory,
    tasks: &BTreeMap<String, Task>,
    my_turn: u32,
    now: UnixMs,
) -> FactoryView {
    let today = now / DAY_MS;
    let mut flow = Flow::default();
    let mut columns: BTreeMap<Column, Vec<(&Task, CardView)>> = BTreeMap::new();
    let mut cancelled = Vec::new();
    for task in tasks.values() {
        let card = card_view(factory, task, tasks, now);
        match task.state.column() {
            Some(Column::Drafting) => flow.drafting += 1,
            Some(Column::Waiting) => flow.waiting += 1,
            Some(Column::Running) => flow.running += 1,
            Some(Column::Done) if task.done_at.is_some_and(|at| at / DAY_MS == today) => {
                flow.done_today += 1;
            }
            Some(Column::Done) => {}
            None => {}
        }
        match task.state.column() {
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
                b_view.needs_person.cmp(&a_view.needs_person).then_with(|| {
                    if a_view.needs_person {
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
        flow,
        my_turn,
        columns,
        cancelled,
        graph,
        dependencies,
        outside_read_at: factory.outside_read_at,
        stale: factory.outside_read_failures >= 3,
        main_broken: factory.main.broken,
        auto_merge_available: factory.config.verification.exists(),
        merge_mode: match factory.config.merge_mode {
            MergeMode::Auto if factory.config.verification.exists() => "auto".into(),
            _ => "manual".into(),
        },
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
        column: task.state.column().map(|column| column.as_str().to_owned()),
        title: task.card.title.clone(),
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
        };
        let frees = waiting_on_this(task, tasks);
        for question in task.open_questions() {
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
                log_tail: attempt.log.as_deref().and_then(log_tail),
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
        verification: if factory.config.verification.exists() {
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
    }
}

/// The last few KiB of a local log; a link that is not a file has none.
fn log_tail(path: &str) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(LOG_TAIL as u64)))
        .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    //! The codes stage 2 decodes are a contract. Each list is matched
    //! exhaustively, so a new variant fails to compile here until it is
    //! listed, and then fails until its wire value is pinned below and in
    //! `docs/factory.md`.
    use super::*;

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
            ]
        );
    }
}
