//! `FactorySummary`: the one value the CLI's `status` and `inbox` print and
//! the stage 2 screens draw (D-50). Every number in it is derived here from
//! the store's Tasks (design #4, #10); the shell computes nothing.
//!
//! This type is a contract with stage 2: add fields, never rename or remove.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::dag;
use crate::model::{Column, DAY_MS, Factory, QuestionKind, Task, TaskState, UnixMs};

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
    pub closed: bool,
    pub flow: Flow,
    pub my_turn: u32,
    pub columns: Vec<ColumnView>,
    /// Off the board, revivable for the keep period (D-47).
    pub cancelled: Vec<CardView>,
    pub graph: Graph,
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
    pub display_id: String,
    pub title: String,
    pub state: String,
    pub state_label: String,
    pub needs_person: bool,
    /// What a waiting Task waits for, in words (D-36).
    pub waiting_for: Option<String>,
    pub priority: i32,
    pub since: UnixMs,
    /// A completion the person has not looked at (D-30).
    pub unread: bool,
    /// Old completions fold (3 days) and old records fold (90 days).
    pub folded: bool,
    pub failures: u32,
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
    pub rank: u8,
    pub factory: String,
    pub task: String,
    pub display_id: String,
    pub title: String,
    pub project: String,
    pub question: Option<String>,
    pub text: String,
    pub suggestion: String,
    pub default_action: Option<String>,
    pub choices: Vec<String>,
    pub deadline: Option<UnixMs>,
    pub remaining: Option<String>,
    pub waiting_since: UnixMs,
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
    FactoryView {
        id: factory.id.clone(),
        project: factory.project.clone(),
        project_name: factory.project_name.clone(),
        source: match factory.source {
            crate::model::SourceKind::Github => "github".into(),
            crate::model::SourceKind::Local => "local".into(),
        },
        closed: factory.closed,
        flow,
        my_turn,
        columns,
        cancelled,
        graph,
        outside_read_at: factory.outside_read_at,
        stale: factory.outside_read_failures >= 3,
        main_broken: factory.main.broken,
        auto_merge_available: factory.config.verification.exists(),
        merge_mode: match factory.config.merge_mode {
            crate::model::MergeMode::Auto if factory.config.verification.exists() => "auto".into(),
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
    let waiting_for = match task.state {
        TaskState::Waiting if !waiting.is_empty() => Some(
            waiting
                .iter()
                .map(|id| {
                    tasks
                        .get(id)
                        .map(Task::display_id)
                        .unwrap_or_else(|| id.clone())
                })
                .collect::<Vec<_>>()
                .join(", "),
        ),
        TaskState::Waiting => task.held.clone().or_else(|| Some("slot".into())),
        TaskState::Blocked => Some(
            task.open_questions()
                .find(|question| matches!(question.kind, QuestionKind::Blocking))
                .map(|_| "answer".to_owned())
                .unwrap_or_else(|| "predecessor".to_owned()),
        ),
        _ => None,
    };
    let folded = match task.state {
        TaskState::Done => task
            .done_at
            .is_some_and(|at| now.saturating_sub(at) > factory.config.done_fold_ms),
        TaskState::Cancelled => task.purged,
        _ => false,
    };
    CardView {
        task: task.id.clone(),
        display_id: task.display_id(),
        title: task.card.title.clone(),
        state: task.state.as_str().to_owned(),
        state_label: task
            .stop
            .filter(|_| task.state == TaskState::Stopped)
            .map(|reason| format!("{} ({})", task.state.label(), reason.label()))
            .unwrap_or_else(|| task.state.label().to_owned()),
        needs_person: task.needs_person(),
        waiting_for,
        priority: task.human.priority,
        since: task.state_since,
        unread: task.state == TaskState::Done && !task.seen,
        folded,
        failures: task.failures,
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
        if matches!(task.state, TaskState::Cancelled) && task.purged {
            continue;
        }
        let base = |group: &str, rank: u8, text: String, since: UnixMs| InboxItem {
            group: group.to_owned(),
            rank,
            factory: factory.id.clone(),
            task: task.id.clone(),
            display_id: task.display_id(),
            title: task.card.title.clone(),
            project: factory.project_name.clone(),
            question: None,
            text,
            suggestion: String::new(),
            default_action: None,
            choices: Vec::new(),
            deadline: None,
            remaining: None,
            waiting_since: since,
        };
        for question in task.open_questions() {
            let (group, rank) = match question.kind {
                QuestionKind::Blocking => ("answer", 0),
                QuestionKind::Notice => ("notice", 4),
                QuestionKind::Action | QuestionKind::Proposal { .. } => ("stopped", 3),
                _ => ("answer", 1),
            };
            let mut item = base(group, rank, question.text.clone(), question.asked_at);
            item.question = Some(question.id.clone());
            item.suggestion = question.suggestion.clone();
            item.default_action = question.default_action.clone();
            item.choices = question.choices.clone();
            item.deadline = question.deadline;
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
                    format!(
                        "멈춤: {}",
                        task.stop.map(|reason| reason.label()).unwrap_or("?")
                    ),
                    task.state_since,
                );
                item.choices = vec!["retry".into(), "cancel".into()];
                items.push(item);
            }
            _ => {}
        }
    }
    items
}
