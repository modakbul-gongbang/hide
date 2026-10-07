//! What the Factory screens read and send (PRD software-factory-ui).
//!
//! The engine's `FactorySummary` and the open Task page's `TaskDetail` are
//! built on the engine thread, compared there with what was last handed to
//! the runtime, and handed over only when they differ, so an idle tick
//! publishes nothing (B25) and nothing is built or compared under
//! `Mutex<Runtime>`. A screen action is the stage-1 command itself, run with
//! the operator role the screen holds; its answer comes back by request id.

use std::collections::VecDeque;
use std::sync::Arc;

use hide_factory::Command;
use hide_factory::summary::{FactorySummary, TaskDetail};
use serde::{Serialize, Serializer};
use serde_json::Value;

/// Who an answer or decision made from the screen was relayed by.
pub const SCREEN_OPERATOR: &str = "screen";
/// The answers the shell can still read for its requests.
pub const ACTION_ANSWER_LIMIT: usize = 16;
/// A shell's request id; longer ids are refused.
pub const REQUEST_ID_LIMIT: usize = 64;

/// The `factory` snapshot section.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct FactorySection {
    /// Absent until the engine first answers; the screen draws its skeleton
    /// meanwhile (B21).
    #[serde(serialize_with = "optional_arc")]
    pub summary: Option<Arc<FactorySummary>>,
    /// The latest answers to the screen's own requests, newest last.
    pub actions: VecDeque<ActionAnswer>,
}

/// The engine's answer to one screen request: the stage-1 command answer as
/// it is (`ok`, or `reason`, `next_action` and `detail`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ActionAnswer {
    pub request_id: String,
    pub answer: Value,
}

/// The `factory_task` snapshot section: the Task page a screen opened.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FactoryTaskSection {
    pub factory: String,
    pub task: String,
    /// `None` when the engine holds no such Task.
    #[serde(serialize_with = "optional_arc")]
    pub detail: Option<Arc<TaskDetail>>,
}

fn optional_arc<T: Serialize, S: Serializer>(
    value: &Option<Arc<T>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value.as_deref().serialize(serializer)
}

impl FactorySection {
    /// Keeps the newest answers within the cap (rule 15: a cap, never a
    /// larger number).
    pub fn push_answer(&mut self, answer: ActionAnswer) {
        self.actions.push_back(answer);
        while self.actions.len() > ACTION_ANSWER_LIMIT {
            self.actions.pop_front();
        }
    }
}

/// A request a screen sends the engine thread. It rides the host's
/// `Request` beside the CLI's `Command`, which is as large, so boxing the
/// command here would only move the size difference there.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ScreenRequest {
    Action {
        request_id: String,
        command: Command,
    },
    OpenTask {
        factory: String,
        task: String,
    },
    CloseTask,
}

/// The verbs a screen sends: a person's actions, the settings with their
/// checks, and the creation preview. A worker's reports and intake stay with the CLI (D-12).
pub fn screen_may_send(command: &Command) -> bool {
    matches!(
        command,
        Command::Init { .. }
            | Command::Answer { .. }
            | Command::Priority { .. }
            | Command::Dep { remove: true, .. }
            | Command::Pause { .. }
            | Command::Resume { .. }
            | Command::Retry { .. }
            | Command::Merge { .. }
            | Command::RequestChanges { .. }
            | Command::Cancel { .. }
            | Command::Revive { .. }
            | Command::Config { .. }
            | Command::Close { .. }
            | Command::Check { .. }
    )
}

/// What the publisher reads.
pub trait ScreenSource {
    fn summary(&self) -> FactorySummary;
    fn show(&self, factory: &str, task: &str) -> Option<TaskDetail>;
}

impl ScreenSource for hide_factory::Engine {
    fn summary(&self) -> FactorySummary {
        hide_factory::Engine::summary(self)
    }

    fn show(&self, factory: &str, task: &str) -> Option<TaskDetail> {
        hide_factory::Engine::show(self, factory, task)
    }
}

/// Where the publisher hands a changed value: one short lock on the
/// runtime and one announcement.
pub trait ScreenSink {
    /// `summary` is `Some` when it changed; `task` is `Some` when the open
    /// Task page changed, and `Some(None)` when it closed.
    fn publish(
        &mut self,
        summary: Option<Arc<FactorySummary>>,
        task: Option<Option<FactoryTaskSection>>,
    );
    fn answered(&mut self, answer: ActionAnswer);
}

/// The engine thread's side of the screens.
#[derive(Default)]
pub struct Publisher {
    last: Option<Arc<FactorySummary>>,
    open: Option<(String, String)>,
    /// The open page is read again at the next publish.
    task_dirty: bool,
    last_task: Option<FactoryTaskSection>,
}

impl Publisher {
    pub fn open(&mut self, factory: String, task: String) {
        self.open = Some((factory, task));
        self.task_dirty = true;
    }

    pub fn close(&mut self) {
        self.open = None;
        self.task_dirty = true;
    }

    /// An action can change the open page without changing the summary
    /// (a decision, a comment); read it again.
    pub fn touched(&mut self) {
        self.task_dirty = true;
    }

    /// Builds the summary and, when it or the open page moved, hands the
    /// new values to the sink. `source` is `None` while no store exists,
    /// which reads as a machine with no Factory (B1).
    pub fn publish(&mut self, source: Option<&dyn ScreenSource>, sink: &mut dyn ScreenSink) {
        let summary = source.map(ScreenSource::summary).unwrap_or_default();
        let summary_changed = self.last.as_deref() != Some(&summary);
        let summary = summary_changed.then(|| {
            let summary = Arc::new(summary);
            self.last = Some(Arc::clone(&summary));
            summary
        });
        let mut task = None;
        if summary_changed || self.task_dirty {
            self.task_dirty = false;
            let next = self.open.as_ref().map(|(factory, id)| FactoryTaskSection {
                factory: factory.clone(),
                task: id.clone(),
                detail: source
                    .and_then(|source| source.show(factory, id))
                    .map(Arc::new),
            });
            if next != self.last_task {
                self.last_task.clone_from(&next);
                task = Some(next);
            }
        }
        if summary.is_some() || task.is_some() {
            sink.publish(summary, task);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Source {
        summary: RefCell<FactorySummary>,
        detail: RefCell<Option<TaskDetail>>,
        shows: RefCell<u32>,
    }

    impl ScreenSource for Source {
        fn summary(&self) -> FactorySummary {
            self.summary.borrow().clone()
        }
        fn show(&self, _factory: &str, _task: &str) -> Option<TaskDetail> {
            *self.shows.borrow_mut() += 1;
            self.detail.borrow().clone()
        }
    }

    #[derive(Default)]
    struct Sink {
        summaries: Vec<Arc<FactorySummary>>,
        tasks: Vec<Option<FactoryTaskSection>>,
        answers: Vec<ActionAnswer>,
    }

    impl ScreenSink for Sink {
        fn publish(
            &mut self,
            summary: Option<Arc<FactorySummary>>,
            task: Option<Option<FactoryTaskSection>>,
        ) {
            self.summaries.extend(summary);
            self.tasks.extend(task);
        }
        fn answered(&mut self, answer: ActionAnswer) {
            self.answers.push(answer);
        }
    }

    fn source(my_turn: u32) -> Source {
        Source {
            summary: RefCell::new(FactorySummary {
                my_turn,
                ..FactorySummary::default()
            }),
            detail: RefCell::new(None),
            shows: RefCell::new(0),
        }
    }

    #[test]
    fn a_machine_without_a_store_publishes_one_empty_summary_and_then_nothing() {
        let mut publisher = Publisher::default();
        let mut sink = Sink::default();
        publisher.publish(None, &mut sink);
        publisher.publish(None, &mut sink);
        publisher.publish(None, &mut sink);
        assert_eq!(sink.summaries.len(), 1, "the empty summary once");
        assert_eq!(*sink.summaries[0], FactorySummary::default());
        assert!(sink.tasks.is_empty(), "no page was opened");
    }

    #[test]
    fn an_unchanged_tick_publishes_nothing_and_a_transition_publishes_once() {
        let mut publisher = Publisher::default();
        let mut sink = Sink::default();
        let engine = source(2);
        for _ in 0..5 {
            publisher.publish(Some(&engine), &mut sink);
        }
        assert_eq!(sink.summaries.len(), 1, "idle ticks publish nothing (B25)");
        engine.summary.borrow_mut().my_turn = 1;
        publisher.publish(Some(&engine), &mut sink);
        publisher.publish(Some(&engine), &mut sink);
        assert_eq!(sink.summaries.len(), 2, "one transition, one publish");
        assert_eq!(sink.summaries[1].my_turn, 1);
    }

    #[test]
    fn an_open_page_is_read_on_open_and_on_a_transition_only() {
        let mut publisher = Publisher::default();
        let mut sink = Sink::default();
        let engine = source(0);
        publisher.publish(Some(&engine), &mut sink);
        publisher.open("f-1".into(), "T-1".into());
        publisher.publish(Some(&engine), &mut sink);
        assert_eq!(*engine.shows.borrow(), 1);
        assert_eq!(
            sink.tasks,
            vec![Some(FactoryTaskSection {
                factory: "f-1".into(),
                task: "T-1".into(),
                detail: None,
            })],
            "a missing Task is a page with no detail"
        );
        publisher.publish(Some(&engine), &mut sink);
        assert_eq!(*engine.shows.borrow(), 1, "an idle tick reads no page");
        engine.summary.borrow_mut().my_turn = 3;
        publisher.publish(Some(&engine), &mut sink);
        assert_eq!(*engine.shows.borrow(), 2, "a transition reads the page");
        assert_eq!(sink.tasks.len(), 1, "an unchanged page is not handed over");
        publisher.close();
        publisher.publish(Some(&engine), &mut sink);
        assert_eq!(sink.tasks.last(), Some(&None), "closing clears the page");
    }

    #[test]
    fn the_screen_sends_a_persons_actions_and_never_a_workers_reports() {
        let done = Command::Done {
            summary: None,
            breaking: false,
            letter: None,
        };
        let add = Command::Add {
            project: None,
            task: None,
            issue: None,
            card: Default::default(),
            producer_pane: None,
        };
        let dep_add = Command::Dep {
            task: "T-1".into(),
            on: "T-2".into(),
            remove: false,
        };
        assert!(!screen_may_send(&done));
        assert!(!screen_may_send(&add), "no add button in v1 (D-12)");
        assert!(!screen_may_send(&dep_add), "a person only loosens an order");
        assert!(screen_may_send(&Command::Merge { task: "T-1".into() }));
        assert!(screen_may_send(&Command::Dep {
            task: "T-1".into(),
            on: "T-2".into(),
            remove: true,
        }));
    }

    #[test]
    fn the_answers_a_screen_reads_are_capped() {
        let mut section = FactorySection::default();
        for n in 0..(ACTION_ANSWER_LIMIT + 5) {
            section.push_answer(ActionAnswer {
                request_id: n.to_string(),
                answer: Value::Null,
            });
        }
        assert_eq!(section.actions.len(), ACTION_ANSWER_LIMIT);
        assert_eq!(section.actions.front().unwrap().request_id, "5");
    }
}
