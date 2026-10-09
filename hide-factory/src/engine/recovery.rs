//! The recovery schedule (D-30, D-44): every hold a Factory meets, the
//! machine holding new starts, a cascade halting them, outside reads that
//! keep failing, a Task stopped for a reason a restart can clear, gets one
//! action of the closed list at 30, 90 and 150 minutes and becomes a
//! person's at 180. Each action is a line of the activity log, first as
//! running and then with what came of it (B15); a command outside the list
//! is a person's to-do (B16).
//!
//! Holds live in the Factory record, so a restart keeps their clock; which
//! condition is present is read again from the engine each tick.

use std::collections::BTreeSet;

use serde_json::json;

use super::{Engine, Purpose};
use crate::judgment::{self, Judgment, JudgmentInput, JudgmentOutcome, Priority};
use crate::model::*;
use crate::words::Language;

/// When each automatic step runs, from the hold's start (D-44).
pub(super) const STEPS_MS: [u64; 3] = [30 * MINUTE_MS, 90 * MINUTE_MS, 150 * MINUTE_MS];
/// When a hold the steps did not clear becomes a person's.
pub(super) const ESCALATE_MS: u64 = 180 * MINUTE_MS;
/// How long an action has to clear its hold before the log says it did not.
const SETTLE_MS: u64 = 5 * MINUTE_MS;
/// Outside reads failing this many times in a row hold a Factory's reads.
const READ_FAILURES: u32 = 3;
/// The most command to-dos a Factory keeps unresolved.
const COMMAND_LIMIT: usize = 20;
/// The most resolved command to-dos a Factory keeps for its record.
const RESOLVED_COMMANDS_KEPT: usize = 50;

/// What one recovery action did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Effect {
    /// It changed something; an action with nothing to act on did not.
    pub acted: bool,
    /// The Tasks whose worktrees it removed.
    pub removed: Vec<String>,
    /// The disk it freed, when the machine can say.
    pub freed: Option<u64>,
}

impl Engine {
    /// Opens a hold for each condition that appeared, closes the ones that
    /// cleared, and runs each open hold's next step when it is due.
    pub(super) fn recovery_tick(&mut self) {
        let now = self.now();
        let factories: Vec<String> = self
            .factories
            .values()
            .filter(|f| !f.closed && !f.paused)
            .map(|f| f.id.clone())
            .collect();
        for factory in factories {
            let present = self.present_holds(&factory, now);
            self.open_and_close(&factory, &present, now);
            for key in present {
                self.step_hold(&factory, &key, now);
            }
        }
    }

    /// The holds this Factory meets now.
    fn present_holds(&self, factory: &str, now: UnixMs) -> BTreeSet<HoldKey> {
        let mut keys = BTreeSet::new();
        for task in self.tasks_of(factory) {
            if let Some(hold) = task.held_code
                && matches!(task.state, TaskState::Waiting | TaskState::Relanding)
            {
                keys.insert(HoldKey::Start { hold });
            }
            if task.recoverable_stop() {
                keys.insert(HoldKey::Task {
                    task: task.id.clone(),
                });
            }
        }
        if self.halt_until.is_some_and(|until| until > now)
            && self.halt_factory.as_deref() == Some(factory)
        {
            keys.insert(HoldKey::Halt);
        }
        // Reads a sign-in or permission blocks wait for the person's
        // to-do, not for a recovery (D-46).
        if self
            .factories
            .get(factory)
            .is_some_and(|f| f.outside_read_failures >= READ_FAILURES && f.github_block.is_none())
        {
            keys.insert(HoldKey::Reads);
        }
        keys
    }

    fn open_and_close(&mut self, factory: &str, present: &BTreeSet<HoldKey>, now: UnixMs) {
        let Some(f) = self.factories.get(factory) else {
            return;
        };
        let cleared: Vec<Hold> = f
            .holds
            .iter()
            .filter(|hold| !present.contains(&hold.key))
            .cloned()
            .collect();
        let opened: Vec<HoldKey> = present
            .iter()
            .filter(|key| f.hold(key).is_none())
            .cloned()
            .collect();
        if cleared.is_empty() && opened.is_empty() {
            return;
        }
        for hold in &cleared {
            // The action still being read cleared it.
            if let Some(action) = hold
                .attempts
                .last()
                .filter(|attempt| attempt.outcome.is_none())
                .and_then(|attempt| attempt.action)
            {
                self.log_recovery(
                    factory,
                    &hold.key,
                    action,
                    Some(RecoveryOutcome::Improved),
                    &Effect::default(),
                );
            }
            self.record(
                factory,
                hold_task(&hold.key),
                "recovery.cleared",
                json!({"hold": hold.key, "attempts": hold.attempts.len()}),
            );
        }
        if let Some(f) = self.factories.get_mut(factory) {
            f.holds.retain(|hold| present.contains(&hold.key));
            for key in opened {
                f.holds.push(Hold {
                    key,
                    since: now,
                    attempts: Vec::new(),
                    diagnosing: false,
                    cause: None,
                    escalated: false,
                });
            }
        }
        self.save_factory(factory);
    }

    fn step_hold(&mut self, factory: &str, key: &HoldKey, now: UnixMs) {
        let Some(hold) = self
            .factories
            .get(factory)
            .and_then(|f| f.hold(key))
            .cloned()
        else {
            return;
        };
        // An action that had its time and left the hold in place helped
        // only partly.
        if let Some(last) = hold.attempts.last()
            && last.outcome.is_none()
            && let Some(action) = last.action
            && now.saturating_sub(last.at) >= SETTLE_MS
        {
            self.settle_attempt(factory, key, RecoveryOutcome::Partial);
            self.log_recovery(
                factory,
                key,
                action,
                Some(RecoveryOutcome::Partial),
                &Effect::default(),
            );
        }
        if hold.escalated {
            return;
        }
        let elapsed = now.saturating_sub(hold.since);
        if elapsed >= ESCALATE_MS {
            self.change_hold(factory, key, |hold| {
                hold.escalated = true;
                hold.diagnosing = false;
            });
            self.record(
                factory,
                hold_task(key),
                "recovery.escalated",
                json!({"hold": key}),
            );
            return;
        }
        let step = hold.attempts.len();
        if hold.diagnosing || step >= STEPS_MS.len() || elapsed < STEPS_MS[step] {
            return;
        }
        let actions = self.available_actions(factory, key);
        if actions.is_empty() {
            // Nothing left to try waits for the 180-minute mark (B14).
            return self.run_step(factory, key, None, now);
        }
        if self.ask_recovery(factory, key, &hold, &actions, now) {
            self.change_hold(factory, key, |hold| hold.diagnosing = true);
            return;
        }
        // No diagnosis to ask: the first action not tried yet.
        let action = fallback(&hold, &actions);
        self.run_step(factory, key, Some(action), now);
    }

    /// The enabled actions a diagnosis may pick for this hold, the ones
    /// that fit it first, which is also the order tried without one. A
    /// worker restarts once per Task (D-44).
    fn available_actions(&self, factory: &str, key: &HoldKey) -> Vec<RecoveryAction> {
        use RecoveryAction::*;
        let Some(f) = self.factories.get(factory) else {
            return Vec::new();
        };
        let fits: &[RecoveryAction] = match key {
            HoldKey::Start {
                hold: EnvHold::DiskFloor | EnvHold::DiskFull,
            } => &[RemoveFinishedWorktrees, RetryReadsAndReconnect],
            HoldKey::Start {
                hold: EnvHold::MemoryCritical,
            } => &[SleepWakeWorker, RetryReadsAndReconnect],
            HoldKey::Halt => &[RetryReadsAndReconnect, SwitchRuntime],
            HoldKey::Reads => &[RetryReadsAndReconnect],
            HoldKey::Task { .. } => &[RestartWorker, SwitchRuntime],
        };
        let restarted = match key {
            HoldKey::Task { task } => self
                .task(factory, task)
                .is_some_and(|t| t.recovery_restarted),
            _ => false,
        };
        fits.iter()
            .chain(
                RecoveryAction::ALL
                    .iter()
                    .filter(|action| !fits.contains(action)),
            )
            .copied()
            .filter(|action| f.config.recovery.contains(action))
            .filter(|action| !(*action == RestartWorker && restarted))
            .collect()
    }

    /// Asks which action fits; false when the question cannot be asked.
    fn ask_recovery(
        &mut self,
        factory: &str,
        key: &HoldKey,
        hold: &Hold,
        actions: &[RecoveryAction],
        now: UnixMs,
    ) -> bool {
        let Some(f) = self.factories.get(factory).cloned() else {
            return false;
        };
        let task = hold_task(key)
            .and_then(|id| self.task(factory, id))
            .map(|t| {
                json!({
                    "state": t.state.as_str(),
                    "stop": t.stop,
                    "detail": t.stop_detail.as_deref().map(|d| judgment::cut(d, 300)),
                })
            });
        let facts = json!({
            "hold": key,
            "minutes": now.saturating_sub(hold.since) / MINUTE_MS,
            "tried": hold.attempts.iter().map(|a| json!({
                "action": a.action.map(RecoveryAction::as_str),
                "outcome": a.outcome.map(RecoveryOutcome::as_str),
            })).collect::<Vec<_>>(),
            "task": task,
            "recent_failures": self.env_failures.iter()
                .filter(|note| note.factory == factory)
                .map(|note| json!({"task": note.task, "stage": note.stage, "kind": note.kind.as_str()}))
                .collect::<Vec<_>>(),
            "disk_free": self.ports.environment.disk_free(&f.project),
        });
        let judgment = Judgment {
            id: format!(
                "{factory}:recovery:{}:{}",
                crate::summary::hold_name(key),
                hold.attempts.len()
            ),
            factory: factory.to_owned(),
            task: hold_task(key).map(str::to_owned),
            priority: Priority::Factory,
            input: JudgmentInput::EnvDiagnosis {
                facts,
                actions: actions.to_vec(),
            },
            ai: None,
            language: Language::English,
        };
        let id = judgment.id.clone();
        if self.submit_judgment(judgment).is_err() {
            return false;
        }
        self.judgments.insert(
            id,
            (
                factory.to_owned(),
                None,
                Purpose::Recovery { key: key.clone() },
            ),
        );
        true
    }

    /// The diagnosis answered: its action when it is one this hold can
    /// take, the fallback when it could not answer, and its command, when
    /// it named one outside the list, as a person's to-do (B16).
    pub(super) fn apply_recovery(
        &mut self,
        factory: &str,
        key: &HoldKey,
        outcome: &JudgmentOutcome,
    ) {
        let Some(hold) = self
            .factories
            .get(factory)
            .and_then(|f| f.hold(key))
            .cloned()
        else {
            // The hold cleared while it was asked.
            return;
        };
        self.change_hold(factory, key, |hold| hold.diagnosing = false);
        if hold.escalated {
            return;
        }
        let actions = self.available_actions(factory, key);
        let now = self.now();
        let diagnosis = match outcome {
            JudgmentOutcome::Answered { value } => judgment::parse_env(value),
            JudgmentOutcome::Failed { reason } => Err(reason.clone()),
        };
        match diagnosis {
            Ok(diagnosis) => {
                let cause = judgment::cut(&diagnosis.cause, 600);
                self.change_hold(factory, key, |hold| hold.cause = Some(cause.clone()));
                if let Some((command, impact)) = diagnosis.proposal {
                    self.add_command(factory, &command, &impact, &cause, now);
                }
                let action = diagnosis.action.filter(|action| actions.contains(action));
                self.run_step(factory, key, action, now);
            }
            Err(reason) => {
                self.record(
                    factory,
                    hold_task(key),
                    "recovery.diagnosis_failed",
                    json!({"reason": reason}),
                );
                let action = (!actions.is_empty()).then(|| fallback(&hold, &actions));
                self.run_step(factory, key, action, now);
            }
        }
    }

    fn run_step(
        &mut self,
        factory: &str,
        key: &HoldKey,
        action: Option<RecoveryAction>,
        now: UnixMs,
    ) {
        let effect = match action {
            Some(action) => self.run_recovery(factory, action, hold_task(key)),
            None => Effect::default(),
        };
        // An action with nothing to act on is unchanged at once.
        let outcome = (!effect.acted).then_some(RecoveryOutcome::Unchanged);
        self.change_hold(factory, key, |hold| {
            hold.attempts.push(RecoveryAttempt {
                at: now,
                action,
                outcome: action.and(outcome),
            })
        });
        if let Some(action) = action {
            self.log_recovery(factory, key, action, outcome, &effect);
        }
    }

    fn settle_attempt(&mut self, factory: &str, key: &HoldKey, outcome: RecoveryOutcome) {
        self.change_hold(factory, key, |hold| {
            if let Some(last) = hold.attempts.last_mut() {
                last.outcome = Some(outcome);
            }
        });
    }

    fn change_hold(&mut self, factory: &str, key: &HoldKey, change: impl FnOnce(&mut Hold)) {
        if let Some(hold) = self
            .factories
            .get_mut(factory)
            .and_then(|f| f.holds.iter_mut().find(|hold| &hold.key == key))
        {
            change(hold);
            self.save_factory(factory);
        }
    }

    /// A Task's recovery is a line of its own activity; any other hold's is
    /// the Factory's.
    fn log_recovery(
        &mut self,
        factory: &str,
        key: &HoldKey,
        action: RecoveryAction,
        outcome: Option<RecoveryOutcome>,
        effect: &Effect,
    ) {
        let event = ActivityEvent::Recovery {
            action,
            outcome,
            removed: effect.removed.clone(),
            freed: effect.freed,
        };
        match hold_task(key) {
            Some(task) => self.log_task(factory, task, event),
            None => self.log_factory(factory, None, event),
        }
    }

    /// A command only a person can run, once per command until resolved.
    pub(super) fn add_command(
        &mut self,
        factory: &str,
        command: &str,
        impact: &str,
        cause: &str,
        now: UnixMs,
    ) {
        let Some(command) = copyable_command(command).map(str::to_owned) else {
            self.record(
                factory,
                None,
                "command.refused",
                json!({"bytes": command.len()}),
            );
            return;
        };
        let Some(f) = self.factories.get_mut(factory) else {
            return;
        };
        let open: Vec<&CommandToDo> = f
            .commands
            .iter()
            .filter(|c| c.resolved_at.is_none())
            .collect();
        if open.iter().any(|c| c.command == command) {
            return;
        }
        if open.len() >= COMMAND_LIMIT {
            self.record(factory, None, "command.capped", json!({}));
            return;
        }
        f.next_command += 1;
        f.commands.push(CommandToDo {
            id: format!("C{}", f.next_command),
            command,
            impact: judgment::cut(impact, 600),
            cause: judgment::cut(cause, 600),
            at: now,
            resolved_at: None,
        });
        let resolved = f
            .commands
            .iter()
            .filter(|c| c.resolved_at.is_some())
            .count();
        let mut drop = resolved.saturating_sub(RESOLVED_COMMANDS_KEPT);
        f.commands.retain(|c| {
            let oldest_resolved = drop > 0 && c.resolved_at.is_some();
            drop -= usize::from(oldest_resolved);
            !oldest_resolved
        });
        self.save_factory(factory);
    }

    /// One action of the closed list (D-54), on this Factory's Tasks only;
    /// `task` narrows a worker restart to the Task whose stop it clears.
    pub(super) fn run_recovery(
        &mut self,
        factory: &str,
        action: RecoveryAction,
        task: Option<&str>,
    ) -> Effect {
        self.record(
            factory,
            task,
            "recovery.run",
            json!({"action": action.as_str()}),
        );
        match action {
            RecoveryAction::RemoveFinishedWorktrees => self.clean_worktrees(factory),
            // Reads a backoff holds are asked again, the start hold is read
            // again, and a cascade's halt lifts so the next start shows
            // whether the cause is gone.
            RecoveryAction::RetryReadsAndReconnect => {
                self.github_backoff.remove(factory);
                self.hold_reason = None;
                self.hold_checked_at = None;
                if self.halt_factory.as_deref() == Some(factory) {
                    self.halt_until = None;
                    self.halt_factory = None;
                }
                Effect {
                    acted: true,
                    ..Effect::default()
                }
            }
            // A Task the environment stopped starts its worker again in the
            // same worktree and session (B54), once per Task (D-44).
            RecoveryAction::RestartWorker => {
                let stopped: Vec<String> = self
                    .tasks_of(factory)
                    .filter(|t| t.recoverable_stop() && !t.recovery_restarted)
                    .filter(|t| task.is_none_or(|only| t.id == only))
                    .map(|t| t.id.clone())
                    .collect();
                for id in &stopped {
                    self.with_task(factory, id, |task| {
                        task.environment_failures = 0;
                        task.recovery_restarted = true;
                    });
                    self.record(factory, Some(id), "recovery.restart_worker", json!({}));
                    self.set_state(factory, id, TaskState::Waiting);
                }
                Effect {
                    acted: !stopped.is_empty(),
                    ..Effect::default()
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
                let mut acted = false;
                for (id, worker) in stuck {
                    if self.ports.workers.status(&worker) != crate::adapters::WorkerStatus::Blocked
                    {
                        continue;
                    }
                    acted = true;
                    self.record(factory, Some(&id), "recovery.sleep_wake_worker", json!({}));
                    self.put_to_sleep(factory, &id);
                    self.wake(
                        factory,
                        &id,
                        "Factory: woken again by an automatic recovery. Carry on with the work and report with hide factory done when it is finished.",
                    );
                }
                Effect {
                    acted,
                    ..Effect::default()
                }
            }
            // New starts of an unpinned Task leave the Factory's default
            // runtime for an hour, while the other one is not limited.
            RecoveryAction::SwitchRuntime => {
                let Some(candidates) = self.factories.get(factory).map(|f| f.config.candidates())
                else {
                    return Effect::default();
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
                    return Effect::default();
                }
                let until = now + HOUR_MS;
                let entry = self.runtime_blocked.entry(runtime).or_insert(until);
                *entry = (*entry).max(until);
                Effect {
                    acted: true,
                    ..Effect::default()
                }
            }
        }
    }

    /// Removes this Factory's finished worktrees and says what it freed
    /// (B13).
    fn clean_worktrees(&mut self, factory: &str) -> Effect {
        let project = self.factories.get(factory).map(|f| f.project.clone());
        let before = project
            .as_deref()
            .and_then(|project| self.ports.environment.disk_free(project));
        let removed = self.remove_finished_worktrees(Some(factory));
        let after = project
            .as_deref()
            .and_then(|project| self.ports.environment.disk_free(project));
        Effect {
            acted: !removed.is_empty(),
            freed: before
                .zip(after)
                .map(|(before, after)| after.saturating_sub(before))
                .filter(|_| !removed.is_empty()),
            removed,
        }
    }

    /// A full disk is acted on at once (D-44): every Factory that keeps the
    /// cleanup on removes its finished worktrees, and the log says what that
    /// freed.
    pub(super) fn disk_full(&mut self) {
        let factories: Vec<String> = self
            .factories
            .values()
            .filter(|f| {
                !f.closed
                    && f.config
                        .recovery
                        .contains(&RecoveryAction::RemoveFinishedWorktrees)
            })
            .map(|f| f.id.clone())
            .collect();
        for factory in factories {
            let effect = self.clean_worktrees(&factory);
            // A full disk is signalled again and again; a cleanup with
            // nothing to remove would fill the activity with the same line.
            if !effect.acted {
                self.record(&factory, None, "recovery.nothing_to_clean", json!({}));
                continue;
            }
            self.log_factory(
                &factory,
                None,
                ActivityEvent::Recovery {
                    action: RecoveryAction::RemoveFinishedWorktrees,
                    outcome: Some(RecoveryOutcome::Improved),
                    removed: effect.removed,
                    freed: effect.freed,
                },
            );
        }
    }
}

/// The first action this hold has not tried, or the first of all when
/// every one was tried; `actions` is never empty.
fn fallback(hold: &Hold, actions: &[RecoveryAction]) -> RecoveryAction {
    actions
        .iter()
        .copied()
        .find(|action| !hold.attempts.iter().any(|a| a.action == Some(*action)))
        .unwrap_or(actions[0])
}

fn hold_task(key: &HoldKey) -> Option<&str> {
    match key {
        HoldKey::Task { task } => Some(task),
        _ => None,
    }
}
