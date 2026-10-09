//! The Observer (D-14, D-15): decision requests sorted by one AI call and
//! sent by the Factory's mode, the daily cap, the wake and diagnosis of a
//! quiet worker, the automatic restart of a vanished one, the risk-path merge
//! a 맡김 Factory lets the AI approve, and a Factory's pause.
//!
//! Who answers is this file's table, never the AI's (D-14). Every Observer
//! failure sends the request to a person and leaves its reason in the log
//! (B10); the first recorded answer wins and the other changes nothing.

use serde_json::json;

use hide_agent_adapter::Capability;

use super::{
    Engine, NewQuestion, Purpose, Reply, TEXT_LIMIT, attached_prd, decision_lines, refuse,
};
use crate::adapters::{EnvSignal, Failure};
use crate::judgment::{
    self, Classification, DecisionRequest, Judgment, JudgmentInput, JudgmentOutcome, Priority,
    Verdict, WorkerText, WorkerTextSource,
};
use crate::model::*;
use crate::role::Role;
use crate::words::Language;

/// Failure classes of a call the provider never received, not counted
/// against the daily cap (D-34): the engine's full queue and Hide AI's
/// refusals that come before any model turn (`hide_ai::AiError`). A
/// transient failure, a spent budget or an unavailable provider can follow
/// a turn (Hide AI reports a provider past its restart cap only after the
/// turn answered), so each counts; a paused Factory charges nothing in the
/// first place.
const NOT_SENT: [&str; 6] = [
    "disabled",
    "no_provider",
    "unsupported",
    "queue_full",
    "not_authenticated",
    "usage_limited",
];

const NUDGE: &str = "Factory: your turn ended without a report. Report what you were doing with one of hide factory done, ask, block.";
const DONE_REQUEST: &str = "Factory: if the work is finished, report it with hide factory done --result '<one line>' and --changed, --verified, --unverified. If not, ask with hide factory ask or block.";

/// Where the mode table sends a sorted request (D-14, D-32).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    /// A person answers; `proposal` attaches the Observer's fix as a choice.
    Person { proposal: bool },
    /// The Observer answers; its decision is on the Task's record and
    /// activity in every mode (B21).
    Observer,
    /// The Observer's fix for a wrong card is applied (autonomous E).
    Apply,
}

fn route(mode: ObserverMode, verdict: &Classification) -> Route {
    use DecisionKind::*;
    use ObserverMode::*;
    // Unsure, or any permission signal: a person's (D-14).
    if verdict.ambiguous || verdict.permission_signal {
        return Route::Person { proposal: false };
    }
    match (verdict.kind, mode) {
        (A, _) => Route::Observer,
        (B, Manual) => Route::Person { proposal: false },
        (B, _) => Route::Observer,
        (C, Autonomous) => Route::Observer,
        (C, _) => Route::Person { proposal: false },
        (D, _) => Route::Person { proposal: false },
        (E, Manual) => Route::Person { proposal: false },
        (E, Assist) => Route::Person { proposal: true },
        (E, Autonomous) => Route::Apply,
    }
}

impl Engine {
    /// The local day the daily cap counts by (D-34).
    fn local_day(&self) -> u64 {
        let local = self.now() as i64 + self.ports.clock.utc_offset_ms();
        (local.max(0) as u64) / DAY_MS
    }

    /// Takes one call from today's cap. At the cap nothing is sent, and the
    /// first refusal of the day is a line of the Factory's activity, which
    /// its header marks (B21).
    fn charge_observer(&mut self, factory: &str, task: &str) -> bool {
        let day = self.local_day();
        let Some(f) = self.factories.get_mut(factory) else {
            return false;
        };
        if f.observer_day != day {
            f.observer_day = day;
            f.observer_calls = 0;
        }
        if f.observer_calls >= f.config.observer_daily_limit {
            let limit = f.config.observer_daily_limit;
            let first = f.observer_cap_notice_day != day;
            f.observer_cap_notice_day = day;
            self.save_factory(factory);
            self.record(
                factory,
                Some(task),
                "observer.daily_limit",
                json!({"limit": limit}),
            );
            if first {
                self.log_factory(factory, Some(task), ActivityEvent::DailyLimit { limit });
            }
            return false;
        }
        f.observer_calls += 1;
        self.save_factory(factory);
        true
    }

    /// A call the provider never received gives its count back (D-34).
    pub(super) fn observer_not_sent(&mut self, factory: &str, reason: &str) {
        if !NOT_SENT.contains(&reason) {
            return;
        }
        let day = self.local_day();
        if let Some(f) = self.factories.get_mut(factory)
            && f.observer_day == day
        {
            f.observer_calls = f.observer_calls.saturating_sub(1);
            self.save_factory(factory);
        }
    }

    /// Queues one Observer call; the reason it went nowhere otherwise.
    fn submit_observer(
        &mut self,
        factory: &str,
        task: &str,
        judgment: Judgment,
        purpose: Purpose,
    ) -> Result<(), &'static str> {
        if self.judgments.contains_key(&judgment.id) {
            // The same request asked again is the same judgment (B3).
            return Ok(());
        }
        if self.factories.get(factory).is_none_or(|f| f.paused) {
            return Err("paused");
        }
        if !self.charge_observer(factory, task) {
            return Err("daily_limit");
        }
        let id = judgment.id.clone();
        match self.submit_judgment(judgment) {
            Ok(()) => {
                self.judgments
                    .insert(id, (factory.to_owned(), Some(task.to_owned()), purpose));
                Ok(())
            }
            Err(failure) => {
                // Never sent: a full queue or no judgment thread.
                self.observer_not_sent(factory, "queue_full");
                self.record(
                    factory,
                    Some(task),
                    "observer.not_submitted",
                    json!({"judgment": id, "detail": judgment::cut(&failure.detail, 200)}),
                );
                Err("queue_full")
            }
        }
    }

    /// What every Observer call sees of a Task: its card, its recorded
    /// decisions and its PRD (D-16).
    fn observer_context(&self, task: &Task) -> (Card, Vec<String>, Option<String>) {
        (task.card.clone(), decision_lines(task), attached_prd(task))
    }

    fn set_routing(
        &mut self,
        factory: &str,
        id: &str,
        question: &str,
        change: impl FnOnce(&mut Routing),
    ) {
        let mode = self
            .factories
            .get(factory)
            .map_or(ObserverMode::Assist, |f| f.config.observer_mode);
        self.with_task(factory, id, |task| {
            if let Some(q) = task.questions.iter_mut().find(|q| q.id == question) {
                let routing = q.routing.get_or_insert(Routing {
                    to: RouteTo::Pending,
                    mode,
                    kind: None,
                    reason: None,
                    fallback: None,
                    proposal: None,
                    overridden: false,
                });
                change(routing);
            }
        });
    }

    /// The request waits for a person, with why the Observer did not decide.
    pub(super) fn send_to_person(
        &mut self,
        factory: &str,
        id: &str,
        question: &str,
        fallback: &str,
    ) {
        self.set_routing(factory, id, question, |routing| {
            routing.to = RouteTo::Person;
            routing.fallback = Some(fallback.to_owned());
        });
        self.record(
            factory,
            Some(id),
            "observer.to_person",
            json!({"question": question, "reason": fallback}),
        );
    }

    // ------------------------------------------------------- decision requests

    /// Sends a new decision request to the Observer; it waits for a person
    /// whenever the Observer cannot be asked (B3, B10).
    pub(super) fn route_request(&mut self, factory: &str, id: &str, question: &str) {
        let Some(task) = self.task(factory, id).cloned() else {
            return;
        };
        let Some(q) = task.questions.iter().find(|q| q.id == question).cloned() else {
            return;
        };
        self.set_routing(factory, id, question, |_| {});
        let (card, decisions, attachment) = self.observer_context(&task);
        let judgment = Judgment {
            id: format!("{factory}:{id}:observer:{question}"),
            factory: factory.to_owned(),
            task: Some(id.to_owned()),
            priority: Priority::Factory,
            input: JudgmentInput::ObserverClassify {
                request: DecisionRequest {
                    question: q.text.clone(),
                    choices: q.choices.clone(),
                    suggestion: q.suggestion.clone(),
                    default_action: match q.kind {
                        QuestionKind::Blocking => None,
                        _ => q.default_action.clone(),
                    },
                },
                card,
                decisions,
                attachment,
            },
            ai: None,
            language: Language::English,
        };
        let purpose = Purpose::Classify {
            question: question.to_owned(),
        };
        if let Err(fallback) = self.submit_observer(factory, id, judgment, purpose) {
            self.send_to_person(factory, id, question, fallback);
        }
    }

    pub(super) fn apply_classification(
        &mut self,
        factory: &str,
        id: &str,
        question: &str,
        outcome: &JudgmentOutcome,
    ) {
        let Some(task) = self.task(factory, id).cloned() else {
            return;
        };
        let Some(q) = task.questions.iter().find(|q| q.id == question).cloned() else {
            return;
        };
        if !q.open() {
            // A person answered first; the Observer's verdict changes nothing.
            self.record(
                factory,
                Some(id),
                "observer.late",
                json!({"question": question}),
            );
            return;
        }
        let verdict = match outcome {
            JudgmentOutcome::Answered { value } => {
                judgment::parse_classification(value, &task.card)
            }
            JudgmentOutcome::Failed { reason } => Err(reason.clone()),
        };
        match verdict {
            Ok(verdict) => self.route_verdict(factory, id, &q, verdict),
            Err(reason) => {
                self.record(
                    factory,
                    Some(id),
                    "observer.failed",
                    json!({"question": question, "reason": reason}),
                );
                self.send_to_person(factory, id, question, "failed");
            }
        }
    }

    /// Sends a sorted request where the Factory's mode says (D-14).
    fn route_verdict(
        &mut self,
        factory: &str,
        id: &str,
        question: &Question,
        verdict: Classification,
    ) {
        let mode = question
            .routing
            .as_ref()
            .map(|routing| routing.mode)
            .or_else(|| self.factories.get(factory).map(|f| f.config.observer_mode))
            .unwrap_or_default();
        let kind = verdict.kind;
        let reason = verdict.reason.clone();
        let answer = verdict.answer.clone();
        let chosen = (question.choices.contains(&answer)
            || answer == question.suggestion
            || Some(&answer) == question.default_action.as_ref())
        .then(|| answer.clone());
        let mut route = route(mode, &verdict);
        // A closed question runs only a listed choice; any other words would
        // settle it with nothing run, so they are a person's (D-33).
        if route == Route::Observer
            && (answer.is_empty() || (question.kind.closed() && chosen.is_none()))
        {
            route = Route::Person { proposal: false };
        }
        match route {
            Route::Observer => {
                self.settle_answer(
                    factory,
                    id,
                    question,
                    &answer,
                    chosen,
                    OBSERVER,
                    Some((kind, reason.clone())),
                );
                self.record(
                    factory,
                    Some(id),
                    "observer.answered",
                    json!({"question": question.id, "kind": kind.as_str()}),
                );
                self.log_task(
                    factory,
                    id,
                    ActivityEvent::AiDecision {
                        text: format!(
                            "{} -> {}",
                            judgment::cut(&question.text, 200),
                            judgment::cut(&answer, 200)
                        ),
                    },
                );
            }
            Route::Apply => {
                let applied = match verdict.proposal.clone() {
                    Some(proposal) => self.apply_proposal(
                        factory,
                        id,
                        question,
                        proposal,
                        OBSERVER,
                        Some((kind, reason.clone())),
                    ),
                    None => false,
                };
                if !applied {
                    self.person_with_verdict(factory, id, &question.id, &verdict, false);
                }
            }
            Route::Person { proposal } => {
                self.person_with_verdict(factory, id, &question.id, &verdict, proposal)
            }
        }
    }

    fn person_with_verdict(
        &mut self,
        factory: &str,
        id: &str,
        question: &str,
        verdict: &Classification,
        attach: bool,
    ) {
        let proposal = verdict.proposal.clone().filter(|_| attach);
        let offered = proposal.is_some();
        // Unsure or a permission is a person's whatever the kind and mode,
        // so the kind's line in the mode table is not why (B7).
        let fallback = if verdict.permission_signal {
            Some("permission")
        } else if verdict.ambiguous {
            Some("unsure")
        } else {
            None
        };
        self.set_routing(factory, id, question, |routing| {
            routing.to = RouteTo::Person;
            routing.kind = Some(verdict.kind);
            routing.reason = Some(verdict.reason.clone());
            routing.fallback = fallback.map(str::to_owned);
            routing.proposal = proposal;
        });
        // What it holds up and where each choice leads, in the person's
        // language, where the asker left them out (D-33).
        self.with_task(factory, id, |task| {
            if let Some(q) = task.questions.iter_mut().find(|q| q.id == question) {
                if q.stopped.is_none() {
                    q.stopped = verdict.stopped.clone();
                }
                if q.outcomes.is_empty() {
                    q.outcomes = verdict
                        .outcomes
                        .iter()
                        .filter(|outcome| q.choices.contains(&outcome.choice))
                        .cloned()
                        .collect();
                }
            }
        });
        if offered {
            self.with_task(factory, id, |task| {
                if let Some(q) = task.questions.iter_mut().find(|q| q.id == question)
                    && !q.choices.iter().any(|c| c == PROPOSAL_CHOICE)
                {
                    q.choices.push(PROPOSAL_CHOICE.to_owned());
                }
            });
        }
        self.record(
            factory,
            Some(id),
            "observer.to_person",
            json!({"question": question, "kind": verdict.kind.as_str()}),
        );
    }

    /// Applies the Observer's fix for a wrong card through the path a
    /// person's approval takes (D-33, D-38); false when it cannot be applied.
    pub(super) fn apply_proposal(
        &mut self,
        factory: &str,
        id: &str,
        question: &Question,
        proposal: ObserverProposal,
        by: &str,
        observer: Option<(DecisionKind, String)>,
    ) -> bool {
        let Some(task) = self.task(factory, id).cloned() else {
            return false;
        };
        let Some(config) = self.factories.get(factory).map(|f| f.config.clone()) else {
            return false;
        };
        let now = self.now();
        let blocking = matches!(question.kind, QuestionKind::Blocking);
        let (text, body, prerequisite) = match &proposal {
            ObserverProposal::CardFix { card } => {
                let card = (**card).clone();
                self.with_task(factory, id, |task| {
                    task.card = card.clone();
                    // An approved scope change waits for a person's merge (B8).
                    task.scope_approved = true;
                });
                (
                    format!("카드 고침: {}", card.title),
                    format!(
                        "Factory: 카드가 고쳐졌습니다. 새 카드로 진행하세요.\n새 카드:\n{}",
                        super::card_text(&card)
                    ),
                    false,
                )
            }
            ObserverProposal::NewTask { card, prerequisite } => {
                // At the new-Task cap the Observer applies nothing (D-38),
                // and a Task that was itself proposed or started by autonomy
                // proposes no Task, as a worker's own proposal is refused.
                if observer.is_some()
                    && ((task.new_tasks >= config.new_task_limit && !task.new_task_cap_extended)
                        || task.proposed_by.is_some()
                        || task.autonomy.is_some())
                {
                    return false;
                }
                let new_id = self.create_child(factory, id, (**card).clone(), None);
                self.with_task(factory, id, |task| task.new_tasks += 1);
                if *prerequisite {
                    let discovery = format!(
                        "D{}",
                        self.task(factory, id).map_or(0, |t| t.discoveries.len()) + 1
                    );
                    self.with_task(factory, id, |task| {
                        task.discoveries.push(Discovery {
                            id: discovery.clone(),
                            class: DiscoveryClass::Prerequisite,
                            text: judgment::cut(&card.title, TEXT_LIMIT),
                            at: now,
                            task: None,
                            follow_up: None,
                        })
                    });
                    self.wait_on_prerequisite(factory, id, &discovery, &new_id);
                }
                let after = if *prerequisite {
                    "그 Task가 머지된 뒤 이어서 진행합니다."
                } else {
                    "이 Task는 지금 범위로 계속하세요."
                };
                (
                    format!("새 Task {new_id}: {}", card.title),
                    format!(
                        "Factory: 새 Task {new_id}({})를 만들었습니다. {after}",
                        card.title
                    ),
                    *prerequisite,
                )
            }
        };
        let answer = Answer {
            text: text.clone(),
            chose: Some(PROPOSAL_CHOICE.to_owned()),
            relayed_by: by.to_owned(),
            at: now,
        };
        self.with_task(factory, id, |task| {
            if let Some(q) = task.questions.iter_mut().find(|q| q.id == question.id) {
                q.answer = Some(answer.clone());
                if let Some(routing) = &mut q.routing
                    && let Some((kind, reason)) = &observer
                {
                    routing.to = RouteTo::Observer;
                    routing.kind = Some(*kind);
                    routing.reason = Some(reason.clone());
                }
            }
            // An applied fix is on the record as it was applied; it is
            // undone its own way, never by a different answer (B29).
            task.decisions.push(DecisionRecord {
                kind: observer.as_ref().map(|(kind, _)| *kind),
                reason: observer.as_ref().map(|(_, reason)| reason.clone()),
                question: Some(question.id.clone()),
                ..DecisionRecord::new(
                    format!("{} -> {text}", judgment::cut(&question.text, 200)),
                    by.to_owned(),
                    now,
                )
            });
            if observer.is_none() {
                task.person_items += 1;
            }
        });
        self.record(
            factory,
            Some(id),
            "proposal.applied",
            json!({"question": question.id, "by": by}),
        );
        self.reply(factory, id, question.letter.as_deref(), &body);
        if blocking
            && !prerequisite
            && self.task(factory, id).is_some_and(|t| {
                t.state == TaskState::Blocked
                    && !t
                        .open_questions()
                        .any(|q| matches!(q.kind, QuestionKind::Blocking))
            })
        {
            self.set_state(factory, id, TaskState::Waiting);
        }
        if observer.is_some() {
            self.log_task(factory, id, ActivityEvent::AiDecision { text });
        }
        true
    }

    /// "다른 답" on a decision the Observer made (D-19, B12).
    pub(super) fn override_answer(
        &mut self,
        role: &Role,
        factory: &str,
        id: &str,
        question: Question,
        choice: Option<String>,
        text: Option<String>,
    ) -> Reply {
        let task = self
            .task(factory, id)
            .cloned()
            .ok_or_else(|| refuse("task_not_found", "Check hide factory status"))?;
        if question
            .answer
            .as_ref()
            .is_none_or(|answer| answer.relayed_by != OBSERVER)
        {
            return Err(refuse(
                "already_answered",
                "This question was answered already; only an answer Factory AI gave can be changed",
            ));
        }
        if matches!(
            task.state,
            TaskState::Done | TaskState::Landed | TaskState::Cancelled | TaskState::Outside
        ) {
            return Err(refuse(
                "task_finished",
                "A finished Task's decisions stay as they are",
            )
            .with(json!({"state": task.state.label()})));
        }
        let chosen = match choice.as_deref() {
            Some("suggestion") => Some(question.suggestion.clone()),
            Some("default") => question.default_action.clone(),
            Some(other) => Some(other.to_owned()),
            None => None,
        };
        let text = text.or_else(|| chosen.clone()).unwrap_or_default();
        if text.trim().is_empty() {
            return Err(refuse(
                "answer_required",
                "Pass --choose <choice> or --text with the answer that replaces Factory AI's",
            ));
        }
        let now = self.now();
        let by = role.relayed_by();
        let answer = Answer {
            text: judgment::cut(&text, TEXT_LIMIT),
            chose: chosen,
            relayed_by: by.clone(),
            at: now,
        };
        self.with_task(factory, id, |task| {
            if let Some(q) = task.questions.iter_mut().find(|q| q.id == question.id) {
                q.answer = Some(answer.clone());
                if let Some(routing) = &mut q.routing {
                    routing.overridden = true;
                }
            }
            // Factory AI's decision becomes the person's (B28).
            let text = format!("{} -> {}", judgment::cut(&question.text, 200), answer.text);
            match task
                .decisions
                .iter_mut()
                .rev()
                .find(|d| d.question.as_deref() == Some(question.id.as_str()) && d.by_ai())
            {
                Some(record) => change_record(record, text, &by, now),
                None => task.decisions.push(
                    DecisionRecord {
                        question: Some(question.id.clone()),
                        ..DecisionRecord::new(text, by.clone(), now)
                    }
                    .with_source(DecisionSource::Answer),
                ),
            }
        });
        self.record(
            factory,
            Some(id),
            "question.overridden",
            json!({"question": question.id, "by": by}),
        );
        let body = format!(
            "Factory: a person changed Factory AI's answer.\nQuestion: {}\nNew answer: {}\nWork to the new answer, then report again with hide factory done.",
            judgment::cut(&question.text, 400),
            answer.text
        );
        self.tell_changed(factory, id, &task, question.letter.as_deref(), &body);
        Ok(self.task_answer(factory, id, "answer changed"))
    }

    /// "다른 답" on one of Factory AI's decisions by its id (B28): an answer
    /// goes through the question it answered; an intake assumption or a
    /// send-back is rewritten as the person's.
    pub(super) fn override_decision(
        &mut self,
        role: &Role,
        factory: &str,
        id: &str,
        decision: &str,
        choice: Option<String>,
        text: Option<String>,
    ) -> Reply {
        let task = self
            .task(factory, id)
            .cloned()
            .ok_or_else(|| refuse("task_not_found", "Check hide factory status"))?;
        let Some(record) = decision_index(decision).and_then(|index| task.decisions.get(index))
        else {
            return Err(refuse(
                "decision_not_found",
                "Name a decision as hide factory show lists it, R<n>",
            ));
        };
        if !record.overridable() {
            return Err(refuse(
                "decision_not_changeable",
                "Only an answer, an assumption or a send-back Factory AI made can be changed",
            ));
        }
        if let Some(question) = record
            .question
            .as_ref()
            .and_then(|qid| task.questions.iter().find(|q| &q.id == qid))
            .filter(|q| !q.open())
        {
            return self.override_answer(role, factory, id, question.clone(), choice, text);
        }
        if matches!(
            task.state,
            TaskState::Done | TaskState::Landed | TaskState::Cancelled | TaskState::Outside
        ) {
            return Err(refuse(
                "task_finished",
                "A finished Task's decisions stay as they are",
            )
            .with(json!({"state": task.state.label()})));
        }
        let text = text.or(choice).unwrap_or_default();
        if text.trim().is_empty() {
            return Err(refuse(
                "answer_required",
                "Pass --text with the decision that replaces Factory AI's",
            ));
        }
        let text = judgment::cut(&text, TEXT_LIMIT);
        let now = self.now();
        let by = role.relayed_by();
        let index = decision_index(decision).unwrap_or_default();
        let from = record.text.clone();
        self.with_task(factory, id, |task| {
            if let Some(record) = task.decisions.get_mut(index) {
                change_record(record, text.clone(), &by, now);
            }
        });
        self.record(
            factory,
            Some(id),
            "decision.overridden",
            json!({"decision": decision, "by": by}),
        );
        let body = format!(
            "Factory: a person changed one of Factory AI's decisions.\nWas: {}\nNow: {text}\nWork to the new decision, then report again with hide factory done.",
            judgment::cut(&from, 400)
        );
        self.tell_changed(factory, id, &task, None, &body);
        Ok(self.task_answer(factory, id, "decision changed"))
    }

    /// A worker already at work hears of a changed decision; one that
    /// reported is sent back with it. A Task not started reads it in its
    /// first prompt.
    fn tell_changed(
        &mut self,
        factory: &str,
        id: &str,
        task: &Task,
        letter: Option<&str>,
        body: &str,
    ) {
        match task.state {
            TaskState::Verifying | TaskState::MergeWaiting => {
                self.cancel_verification(factory, id);
                self.set_state(factory, id, TaskState::Running);
                self.wake(factory, id, body);
            }
            TaskState::Drafting | TaskState::Waiting => {}
            _ => self.reply(factory, id, letter, body),
        }
    }

    /// A person's choice of the Task's worker candidate (D-41).
    pub(super) fn pin_worker(&mut self, factory: &str, id: &str, worker: Option<usize>) -> Reply {
        let f = self
            .factories
            .get(factory)
            .cloned()
            .ok_or_else(|| refuse("factory_not_found", "Check hide factory status"))?;
        let index = super::worker_index(&f, worker)?;
        self.with_task(factory, id, |task| {
            task.human.worker = index;
            task.human.runtime = None;
            // A worker that already runs keeps its candidate; the next new
            // worker takes the chosen one.
            if task.worker.is_none() {
                task.launched = None;
            }
        });
        Ok(self.task_answer(factory, id, "worker candidate set"))
    }

    // ------------------------------------------------------------ quiet worker

    /// A worker resting without a report: one wake, then one diagnosis, then
    /// a person (D-22, D-23, D-24, D-35).
    pub(super) fn check_rest(
        &mut self,
        factory: &str,
        id: &str,
        worker: &WorkerRef,
        rest: UnixMs,
        no_report: u64,
        now: UnixMs,
    ) {
        let Some(task) = self.task(factory, id).cloned() else {
            return;
        };
        // A rest the worker left by working again already moved into
        // `rest_before` (`worker_working`).
        let rest_before = if task.rest_seen == Some(rest) {
            task.rest_before
        } else {
            self.with_task(factory, id, |t| {
                t.rest_before = t.rest_seen.or(t.rest_before);
                t.rest_seen = Some(rest);
            });
            task.rest_seen.or(task.rest_before)
        };
        let started = task.state_since.max(task.woken_at.unwrap_or(0));
        if rest < started {
            return;
        }
        // Only a report made since the rest before this one counts (D-24).
        let baseline = rest_before.filter(|b| *b >= started).unwrap_or(started);
        if task.last_report_at.is_some_and(|at| at >= baseline) {
            if task.recovery.is_some() {
                self.with_task(factory, id, |t| t.recovery = None);
            }
            return;
        }
        let recovery = task.recovery.clone().unwrap_or_default();
        if recovery.diagnosing {
            return;
        }
        let last = recovery.diagnosed_at.or(recovery.woke_at).unwrap_or(0);
        if now.saturating_sub(rest.max(last)) < no_report {
            return;
        }
        // A worker its usage limit stopped waits for a slot and is not the
        // Task's fault (B58).
        if let Some(until) = self.ports.workers.usage_limited(worker.runtime) {
            let mut failure = Failure::environment("worker", EnvSignal::UsageLimit, "usage limit");
            failure.reset_at = Some(until);
            self.external_failure(factory, Some(id), &failure);
            return;
        }
        if recovery.woke_at.is_none() {
            self.with_task(factory, id, |t| {
                t.recovery = Some(Recovery {
                    woke_at: Some(now),
                    ..Recovery::default()
                })
            });
            if self.nudge(factory, id, worker, NUDGE) {
                self.record(factory, Some(id), "worker.nudged", json!({}));
                return;
            }
            // Neither a next-prompt letter nor a resume is declared: the
            // diagnosis comes at once (D-22).
        }
        if recovery.diagnosed_at.is_none() {
            self.diagnose(factory, id, worker);
            return;
        }
        self.stop_no_report(factory, id, None);
    }

    /// A worker working again has left its rest: the Task page stops
    /// counting it, and the rest stays the one before the next (D-24, B43).
    pub(super) fn worker_working(&mut self, factory: &str, id: &str) {
        if self
            .task(factory, id)
            .is_some_and(|t| t.rest_seen.is_some())
        {
            self.with_task(factory, id, |t| {
                t.rest_before = t.rest_seen.take();
            });
        }
    }

    /// Reaches a resting worker the way its adapter declares, never by
    /// typing into its pane (D-22): a next-prompt letter, else a resume of
    /// the same session with the text.
    fn nudge(&mut self, factory: &str, id: &str, worker: &WorkerRef, body: &str) -> bool {
        let capabilities = worker.runtime.adapter().factory;
        let now = self.now();
        let result = if matches!(capabilities.next_prompt_letters, Capability::Available(_)) {
            self.ports
                .workers
                .message(worker, &format!("factory-{id}-nudge-{now}"), None, body)
        } else if matches!(capabilities.resume, Capability::Available(_)) {
            self.ports
                .workers
                .sleep(worker)
                .and_then(|()| self.ports.workers.wake(worker, body))
        } else {
            return false;
        };
        self.keep(factory, Some(id), "letter.out", "nudge", body);
        match result {
            Ok(()) => true,
            Err(failure) => {
                self.external_failure(factory, Some(id), &failure);
                false
            }
        }
    }

    fn diagnose(&mut self, factory: &str, id: &str, worker: &WorkerRef) {
        let now = self.now();
        let Some(task) = self.task(factory, id).cloned() else {
            return;
        };
        // One text, the first the adapter declares and has (D-37).
        let texts = self.ports.workers.texts(worker);
        let capabilities = worker.runtime.adapter().factory;
        let worker_text = [
            (
                matches!(capabilities.user_turn, Capability::Available(_)),
                WorkerTextSource::UserTurn,
                texts.user_turn,
            ),
            (
                matches!(capabilities.turn_end_and_answer, Capability::Available(_)),
                WorkerTextSource::LastAnswer,
                texts.last_answer,
            ),
            (true, WorkerTextSource::Screen, texts.screen),
        ]
        .into_iter()
        .find_map(|(declared, source, text)| {
            text.filter(|text| declared && !text.trim().is_empty())
                .map(|text| WorkerText::bounded(source, &text))
        });
        let read = worker_text.as_ref().map(|text| text.source);
        self.with_task(factory, id, |t| {
            let recovery = t.recovery.get_or_insert_default();
            recovery.diagnosed_at = Some(now);
            recovery.diagnosing = true;
            recovery.diagnosed_from = read;
        });
        let (card, decisions, attachment) = self.observer_context(&task);
        let judgment = Judgment {
            id: format!("{factory}:{id}:diagnose:{now}"),
            factory: factory.to_owned(),
            task: Some(id.to_owned()),
            priority: Priority::Factory,
            input: JudgmentInput::ObserverDiagnose {
                card,
                decisions,
                attachment,
                worker_text,
            },
            ai: None,
            language: Language::English,
        };
        if let Err(reason) = self.submit_observer(factory, id, judgment, Purpose::Diagnose) {
            // No Observer: the card stops for a person at once (B23).
            self.record(
                factory,
                Some(id),
                "observer.to_person",
                json!({"purpose": "diagnose", "reason": reason}),
            );
            self.stop_no_report(factory, id, None);
        }
    }

    pub(super) fn apply_worker_diagnosis(
        &mut self,
        factory: &str,
        id: &str,
        outcome: &JudgmentOutcome,
    ) {
        let Some(task) = self.task(factory, id).cloned() else {
            return;
        };
        self.with_task(factory, id, |t| {
            if let Some(recovery) = &mut t.recovery {
                recovery.diagnosing = false;
            }
        });
        if self.factories.get(factory).is_some_and(|f| f.paused) {
            // Its worker sleeps until the Factory resumes, which reads the
            // rest again from the start (D-48).
            self.with_task(factory, id, |t| t.recovery = None);
            self.record(
                factory,
                Some(id),
                "observer.late",
                json!({"purpose": "diagnose", "reason": "paused"}),
            );
            return;
        }
        let asked = task.recovery.as_ref().and_then(|r| r.diagnosed_at);
        if task.state != TaskState::Running
            || task
                .last_report_at
                .is_some_and(|at| asked.is_some_and(|asked| at >= asked))
        {
            // It reported, or something else moved it, meanwhile.
            return;
        }
        let diagnosis = match outcome {
            JudgmentOutcome::Answered { value } => judgment::parse_diagnosis(value, &task.card),
            JudgmentOutcome::Failed { reason } => Err(reason.clone()),
        };
        let diagnosis = match diagnosis {
            Ok(diagnosis) => diagnosis,
            Err(reason) => {
                self.record(
                    factory,
                    Some(id),
                    "observer.failed",
                    json!({"purpose": "diagnose", "reason": reason}),
                );
                self.stop_no_report(factory, id, None);
                return;
            }
        };
        self.record(
            factory,
            Some(id),
            "observer.diagnosis",
            json!({"verdict": match diagnosis.verdict {
                Verdict::Question { .. } => "question",
                Verdict::ForgotDone => "forgot_done",
                Verdict::Stopped => "stopped",
            }}),
        );
        match diagnosis.verdict {
            Verdict::Question {
                text,
                suggestion,
                choices,
                classification,
            } => {
                // The engine raises the request the worker was asking (B23).
                let deadline = self.now()
                    + self
                        .factories
                        .get(factory)
                        .map_or(24 * HOUR_MS, |f| f.config.question_deadline_ms);
                let mut new = NewQuestion::new(
                    QuestionOrigin::Engine,
                    QuestionKind::Blocking,
                    text,
                    suggestion,
                )
                .deadline(deadline)
                .sorted();
                new.choices = choices;
                let question = self.add_question(factory, id, new);
                self.with_task(factory, id, |t| t.recovery = None);
                self.put_to_sleep(factory, id);
                self.set_state(factory, id, TaskState::Blocked);
                self.set_routing(factory, id, &question, |_| {});
                if let Some(q) = self
                    .task(factory, id)
                    .and_then(|t| t.questions.iter().find(|q| q.id == question).cloned())
                {
                    self.route_verdict(factory, id, &q, classification);
                }
            }
            Verdict::ForgotDone => {
                let now = self.now();
                self.with_task(factory, id, |t| {
                    if let Some(recovery) = &mut t.recovery {
                        recovery.diagnosed_at = Some(now);
                    }
                });
                if let Some(worker) = task.worker.as_ref()
                    && self.nudge(factory, id, worker, DONE_REQUEST)
                {
                    return;
                }
                self.stop_no_report(factory, id, Some(diagnosis.reason));
            }
            Verdict::Stopped => self.stop_no_report(factory, id, Some(diagnosis.reason)),
        }
    }

    fn stop_no_report(&mut self, factory: &str, id: &str, diagnosis: Option<String>) {
        self.set_state(factory, id, TaskState::Stopped);
        self.with_task(factory, id, |task| {
            task.stop = Some(StopReason::NoReport);
            task.diagnosis = diagnosis.filter(|line| !line.is_empty());
            // The wake and diagnosis stay for the Task page's rest record
            // (B43); a retry, a resume or a new start clears them.
            if let Some(recovery) = &mut task.recovery {
                recovery.diagnosing = false;
            }
        });
        self.record(factory, Some(id), "worker.no_report", json!({}));
    }

    /// A worker Hide did not close disappeared: one automatic restart in the
    /// same worktree, then a person (D-25).
    pub(super) fn worker_gone(&mut self, factory: &str, id: &str) {
        let Some(task) = self.task(factory, id).cloned() else {
            return;
        };
        if task.auto_restarts == 0 {
            // A start that waits for its agent's usage reset asks nothing and
            // keeps the one restart; a refused one spends it, so the next
            // disappearance stops the Task for a person (D-25).
            if self.start(factory, id) != super::Start::Waiting {
                self.with_task(factory, id, |t| t.auto_restarts += 1);
                self.record(factory, Some(id), "worker.gone", json!({"restart": true}));
            }
            return;
        }
        self.set_state(factory, id, TaskState::Stopped);
        self.with_task(factory, id, |t| t.stop = Some(StopReason::WorkerGone));
        self.record(factory, Some(id), "worker.gone", json!({"restart": false}));
    }

    /// The operator closed a worker's pane in Hide: its Task pauses and is
    /// not started again until a person resumes it (D-26).
    pub fn worker_closed(&mut self, pane: &str) {
        let closed: Vec<(String, String)> = self
            .all_tasks()
            .filter(|t| matches!(t.state, TaskState::Running | TaskState::Relanding))
            .filter(|t| {
                t.worker
                    .as_ref()
                    .is_some_and(|w| w.pane.as_deref() == Some(pane))
            })
            .map(|t| (t.factory.clone(), t.id.clone()))
            .collect();
        for (factory, id) in closed {
            self.starting.remove(&(factory.clone(), id.clone()));
            self.set_state(&factory, &id, TaskState::Paused);
            self.with_task(&factory, &id, |task| {
                task.pause_reason = Some(PauseReason::PaneClosed);
                task.recovery = None;
            });
            self.record(&factory, Some(&id), "worker.closed", json!({}));
        }
    }

    // --------------------------------------------------------------- merges

    /// Asks the Observer whether a verified Task whose only gate is a risk
    /// path merges, in a 맡김 Factory with Hide AI on (D-21).
    pub(super) fn ask_risk_merge(&mut self, factory: &str, id: &str) {
        let Some(f) = self.factories.get(factory).cloned() else {
            return;
        };
        // Nothing merges on a red main, so its approval is not asked.
        if f.config.observer_mode != ObserverMode::Autonomous
            || f.paused
            || f.closed
            || f.main.broken
        {
            return;
        }
        let Some(task) = self.task(factory, id).cloned() else {
            return;
        };
        let key = format!("observer_merge:{}", task.attempts.len());
        if task.writes.contains(&key) {
            return;
        }
        self.with_task(factory, id, |t| {
            t.writes.insert(key.clone());
        });
        let (card, decisions, attachment) = self.observer_context(&task);
        let judgment = Judgment {
            id: format!("{factory}:{id}:risk-merge:{}", task.attempts.len()),
            factory: factory.to_owned(),
            task: Some(id.to_owned()),
            priority: Priority::Factory,
            input: JudgmentInput::ObserverMerge {
                card,
                decisions,
                attachment,
                risk_paths: f.config.risk_paths.clone(),
            },
            ai: None,
            language: Language::English,
        };
        if let Err(reason) = self.submit_observer(factory, id, judgment, Purpose::RiskMerge) {
            self.record(
                factory,
                Some(id),
                "observer.to_person",
                json!({"purpose": "risk_merge", "reason": reason}),
            );
        }
    }

    pub(super) fn apply_risk_merge(&mut self, factory: &str, id: &str, outcome: &JudgmentOutcome) {
        let Some(task) = self.task(factory, id).cloned() else {
            return;
        };
        let Some(f) = self.factories.get(factory) else {
            return;
        };
        // What allowed the question must still hold when its answer comes:
        // 맡김, an open Factory and a green main; otherwise a person merges.
        if f.config.observer_mode != ObserverMode::Autonomous || f.closed || f.main.broken {
            self.record(
                factory,
                Some(id),
                "observer.late",
                json!({"purpose": "risk_merge", "reason": "factory_changed"}),
            );
            return;
        }
        // A Factory paused meanwhile merges nothing; resuming asks again.
        if f.paused {
            let key = format!("observer_merge:{}", task.attempts.len());
            self.with_task(factory, id, |t| {
                t.writes.remove(&key);
            });
            self.record(
                factory,
                Some(id),
                "observer.late",
                json!({"purpose": "risk_merge", "reason": "paused"}),
            );
            return;
        }
        // A person merged, or the Task moved, first.
        if task.state != TaskState::MergeWaiting || task.gates != [Gate::RiskPath] {
            self.record(
                factory,
                Some(id),
                "observer.late",
                json!({"purpose": "risk_merge"}),
            );
            return;
        }
        let verdict = match outcome {
            JudgmentOutcome::Answered { value } => judgment::parse_merge(value),
            JudgmentOutcome::Failed { reason } => Err(reason.clone()),
        };
        let reason = match verdict {
            Ok((true, reason)) => reason,
            Ok((false, reason)) => {
                self.record(
                    factory,
                    Some(id),
                    "observer.merge_declined",
                    json!({"reason": judgment::cut(&reason, 300)}),
                );
                return;
            }
            Err(reason) => {
                self.record(
                    factory,
                    Some(id),
                    "observer.failed",
                    json!({"purpose": "risk_merge", "reason": reason}),
                );
                return;
            }
        };
        // The path `hide factory merge` takes: the pre-merge check again and
        // the head pinned (D-21).
        match self.manual_merge(&Role::Engine, factory, id) {
            Ok(_) => {
                let now = self.now();
                let reason = judgment::cut(&reason, TEXT_LIMIT);
                self.with_task(factory, id, |t| {
                    t.decisions.push(
                        DecisionRecord {
                            reason: Some(reason.clone()),
                            ..DecisionRecord::new(
                                "risk-path merge approved".into(),
                                OBSERVER.into(),
                                now,
                            )
                        }
                        .with_source(DecisionSource::RiskMerge),
                    )
                });
                self.log_task(factory, id, ActivityEvent::AiDecision { text: reason });
            }
            Err(refusal) => self.record(
                factory,
                Some(id),
                "observer.merge_refused",
                json!({"reason": refusal.reason}),
            ),
        }
    }

    // ---------------------------------------------------------------- pause

    /// Pauses a Factory (D-48): starts, AI judgments and auto merges stop,
    /// and every running worker sleeps where it is. Answers the Tasks whose
    /// worker could not sleep and keeps its turn (D-28).
    pub(super) fn pause_factory(&mut self, factory: &str) -> Vec<String> {
        let Some(f) = self.factories.get_mut(factory) else {
            return Vec::new();
        };
        if f.paused {
            return Vec::new();
        }
        f.paused = true;
        self.save_factory(factory);
        self.record(factory, None, "factory.paused", json!({}));
        let running: Vec<String> = self
            .tasks_of(factory)
            .filter(|t| matches!(t.state, TaskState::Running | TaskState::Relanding))
            .filter(|t| t.worker.as_ref().is_some_and(|w| !w.asleep))
            .map(|t| t.id.clone())
            .collect();
        let mut awake = Vec::new();
        for id in running {
            self.put_to_sleep(factory, &id);
            if self
                .task(factory, &id)
                .and_then(|t| t.worker.as_ref())
                .is_some_and(|w| !w.asleep)
            {
                awake.push(id);
            }
        }
        awake
    }

    /// Resumes a Factory: sleeping workers continue in their worktrees with
    /// what was answered meanwhile, as does a worker that could not sleep and
    /// was sent something, and cards that arrived are reviewed (D-49).
    pub(super) fn resume_factory(&mut self, factory: &str) {
        let Some(f) = self.factories.get_mut(factory) else {
            return;
        };
        if !f.paused {
            return;
        }
        f.paused = false;
        self.save_factory(factory);
        self.record(factory, None, "factory.resumed", json!({}));
        let asleep: Vec<(String, Vec<String>)> = self
            .tasks_of(factory)
            .filter(|t| matches!(t.state, TaskState::Running | TaskState::Relanding))
            .filter_map(|t| {
                let asleep = t.worker.as_ref()?.asleep;
                let pending: Vec<String> = t
                    .flags
                    .iter()
                    .filter_map(|f| f.strip_prefix("pending reply: ").map(str::to_owned))
                    .collect();
                (asleep || !pending.is_empty()).then(|| (t.id.clone(), pending))
            })
            .collect();
        for (id, pending) in asleep {
            let body = if pending.is_empty() {
                "Factory: 다시 시작합니다. 하던 일을 이어서 진행하세요.".to_owned()
            } else {
                pending.join("\n")
            };
            self.with_task(factory, &id, |t| {
                t.flags.retain(|f| !f.starts_with("pending reply: "));
                t.recovery = None;
            });
            self.wake(factory, &id, &body);
        }
        let now = self.now();
        let drafts: Vec<String> = self
            .tasks_of(factory)
            .filter(|t| t.state == TaskState::Drafting && t.review == ReviewState::Pending)
            .map(|t| t.id.clone())
            .collect();
        for id in drafts {
            self.request_review(factory, &id, now);
        }
        // A Task reported while paused runs its checks now, and a verified
        // Task held only by a risk path is asked about again (D-21, D-49).
        let deferred: Vec<String> = self
            .tasks_of(factory)
            .filter(|t| {
                t.state == TaskState::Verifying && t.writes.contains(super::CHECKS_DEFERRED)
            })
            .map(|t| t.id.clone())
            .collect();
        for id in deferred {
            self.start_checks(factory, &id);
        }
        let risk_only: Vec<String> = self
            .tasks_of(factory)
            .filter(|t| t.state == TaskState::MergeWaiting && t.gates == [Gate::RiskPath])
            .map(|t| t.id.clone())
            .collect();
        for id in risk_only {
            self.ask_risk_merge(factory, &id);
        }
    }

    /// Observer calls in flight died with the process that asked them.
    pub(super) fn settle_lost_observer_calls(&mut self) {
        let pending: Vec<(String, String, Vec<String>)> = self
            .all_tasks()
            .map(|t| {
                let questions = t
                    .questions
                    .iter()
                    .filter(|q| q.open())
                    .filter(|q| q.routing.as_ref().is_some_and(|r| r.to == RouteTo::Pending))
                    .map(|q| q.id.clone())
                    .collect::<Vec<_>>();
                (t.factory.clone(), t.id.clone(), questions)
            })
            .filter(|(_, _, questions)| !questions.is_empty())
            .collect();
        for (factory, id, questions) in pending {
            for question in questions {
                self.send_to_person(&factory, &id, &question, "restart");
            }
        }
        let diagnosing: Vec<(String, String)> = self
            .all_tasks()
            .filter(|t| t.recovery.as_ref().is_some_and(|r| r.diagnosing))
            .map(|t| (t.factory.clone(), t.id.clone()))
            .collect();
        for (factory, id) in diagnosing {
            self.with_task(&factory, &id, |t| {
                if let Some(recovery) = &mut t.recovery {
                    recovery.diagnosing = false;
                }
            });
        }
    }
}

/// A person's answer in place of Factory AI's: the record becomes theirs and
/// keeps what Factory AI had decided (B28).
fn change_record(record: &mut DecisionRecord, text: String, by: &str, at: UnixMs) {
    record.changed = Some(DecisionChange {
        by: by.to_owned(),
        at,
        from: std::mem::replace(&mut record.text, text),
    });
    record.by = by.to_owned();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(kind: DecisionKind) -> Classification {
        Classification {
            kind,
            ambiguous: false,
            permission_signal: false,
            answer: "a".into(),
            proposal: None,
            reason: String::new(),
            stopped: None,
            outcomes: Vec::new(),
        }
    }

    /// The table D-14 and D-32 fix, row by row.
    #[test]
    fn the_mode_table_sends_each_kind_where_the_prd_says() {
        use DecisionKind::*;
        use ObserverMode::*;
        let person = Route::Person { proposal: false };
        let ai = Route::Observer;
        let expected = [
            (A, [ai, ai, ai]),
            (B, [person, ai, ai]),
            (C, [person, person, ai]),
            (D, [person, person, person]),
            (E, [person, Route::Person { proposal: true }, Route::Apply]),
        ];
        for (kind, routes) in expected {
            for (mode, expected) in [Manual, Assist, Autonomous].into_iter().zip(routes) {
                assert_eq!(
                    route(mode, &verdict(kind)),
                    expected,
                    "{kind:?} in {mode:?}"
                );
            }
        }
        for mode in [Manual, Assist, Autonomous] {
            let mut unsure = verdict(B);
            unsure.ambiguous = true;
            assert_eq!(route(mode, &unsure), person);
            let mut permission = verdict(A);
            permission.permission_signal = true;
            assert_eq!(route(mode, &permission), person);
        }
    }
}
