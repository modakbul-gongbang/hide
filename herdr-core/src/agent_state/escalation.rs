//! The operator only receives delegated work that its parent cannot handle.
//! Doorbell and watch workers own the clocks; this reads their decisions.
use std::collections::BTreeMap;

use serde::Serialize;

use crate::delivery::doorbell::Hold;
use crate::delivery::ledger::{Ledger, State};
use crate::model::SidebarAgentSnapshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    ParentBlocked,
    Draft,
    BellExhausted,
    Undelivered,
    ChildBlocked,
    ObserverUnconfirmed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Escalation {
    pub cause: Cause,
    pub letter_id: Option<String>,
    pub since_unix_ms: Option<u64>,
    /// The existing delivery human notice already announces these two causes.
    pub human_notice: bool,
}

/// A later waiting reason is not a receipt or successful bell. The worker
/// removes a hold after ringing (or retiring) the letter; until then retain
/// the cause that already raised it. This table remains bounded by the
/// worker's current pending letters and is never used for doorbell verdicts.
pub(crate) fn retain_raised_holds(
    previous: &BTreeMap<String, Hold>,
    mut current: BTreeMap<String, Hold>,
) -> BTreeMap<String, Hold> {
    for (id, old) in previous {
        if matches!(
            old,
            Hold::Blocked | Hold::AwaitingOperator | Hold::Draft | Hold::Exhausted
        ) && let Some(next) = current.get_mut(id)
        {
            *next = *old;
        }
    }
    current
}

pub(crate) fn of(
    child: &SidebarAgentSnapshot,
    ledger: Option<&Ledger>,
    holds: &BTreeMap<String, Hold>,
) -> Option<Escalation> {
    let parent = child.lineage_parent_pane_id.as_deref()?;
    if !child.delegated {
        return None;
    }
    if child.blocked {
        return Some(Escalation {
            cause: Cause::ChildBlocked,
            letter_id: None,
            since_unix_ms: child.changed_at_unix_ms,
            human_notice: false,
        });
    }
    if child.activity == "working" {
        return None;
    }
    let ledger = ledger?;
    for letter in &ledger.letters {
        if matches!(
            letter.state,
            State::Acknowledged | State::Cancelled | State::Expired
        ) || child
            .declared_parent_session
            .as_ref()
            .is_some_and(|session| letter.recipient.session.as_ref() != Some(session))
            || ledger.letters.iter().any(|answer| {
                answer.reply_to.as_deref() == Some(&letter.id)
                    && answer.sender.same_identity(&letter.recipient)
            })
        {
            continue;
        }
        let cause = if let Some(warning) = &letter.watch_warning {
            if warning.ordinal != 1
                || !warning.parent_notified
                || warning.target.pane_id != child.pane_id
                || warning.target.session != child.lineage_session
                || letter.recipient.pane_id != parent
                || !ledger.watches.iter().any(|watch| {
                    watch.target.same_identity(&warning.target)
                        && watch.parent.same_identity(&letter.recipient)
                        && watch.last_activity_at_unix_ms == warning.activity_at_unix_ms
                })
            {
                continue;
            }
            Cause::ObserverUnconfirmed
        } else {
            if letter.sender.pane_id != child.pane_id
                || letter.sender.session != child.lineage_session
                || letter.recipient.pane_id != parent
                || !matches!(letter.kind.as_str(), "request" | "block")
                || took_up_request_after(child, letter.created_at_unix_ms)
            {
                continue;
            }
            if letter.state == State::Undelivered {
                Cause::Undelivered
            } else if letter.state == State::Pending {
                match holds.get(&letter.id) {
                    Some(Hold::Blocked | Hold::AwaitingOperator) => Cause::ParentBlocked,
                    Some(Hold::Draft) => Cause::Draft,
                    Some(Hold::Exhausted) => Cause::BellExhausted,
                    _ => continue,
                }
            } else {
                continue;
            }
        };
        return Some(Escalation {
            cause,
            letter_id: Some(letter.id.clone()),
            since_unix_ms: Some(letter.created_at_unix_ms),
            human_notice: matches!(cause, Cause::Undelivered | Cause::ObserverUnconfirmed),
        });
    }
    None
}

/// Whether the child's session took up a request written after `at`: a new
/// turn means the child moved on without the answer, so its letter no longer
/// raises it, however the turn ends. The time is the session's own record,
/// which the label store keeps across restarts (and reads again for a
/// device), never this daemon's view of the pane.
fn took_up_request_after(child: &SidebarAgentSnapshot, at: u64) -> bool {
    child.row_facts.as_ref().is_some_and(|facts| {
        [&facts.operator_request, &facts.other_request]
            .into_iter()
            .flatten()
            .any(|request| request.at_unix_ms > at)
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RaisedChild {
    pub pane_id: String,
    pub title: String,
    pub tag: super::sessions::Tag,
    pub reason: Option<String>,
    pub since_unix_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delivery::{Actor, mailbox};
    use crate::sidebar::{SessionSnapshotPayload, project_agents};
    use serde_json::json;

    fn child() -> SidebarAgentSnapshot {
        let payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
            {"pane_id":"child", "agent":"codex", "agent_status":"done", "state_change_seq":1}
        ]}))
        .unwrap();
        let mut row = project_agents(payload).agents.remove(0);
        row.delegated = true;
        row.lineage_parent_pane_id = Some("parent".into());
        row.lineage_session = Some("child-session".into());
        row.declared_parent_session = Some("parent-session".into());
        row
    }

    fn actor(pane: &str) -> Actor {
        Actor {
            pane_id: pane.into(),
            name: pane.into(),
            kind: "codex".into(),
            device_id: "local".into(),
            session: Some(format!("{pane}-session")),
        }
    }

    #[test]
    fn session_escalation_clears_on_ack_reply_cancel_or_successful_bell() {
        let child = child();
        for hold in [Hold::Blocked, Hold::Draft, Hold::Exhausted] {
            let mut ledger = Ledger::default();
            mailbox::send(
                &mut ledger,
                &actor("child"),
                &actor("parent"),
                "request",
                "question",
                "request",
                None,
                100,
            )
            .unwrap();
            let id = ledger.letters[0].id.clone();
            let holds = BTreeMap::from([(id.clone(), hold)]);
            assert!(of(&child, Some(&ledger), &holds).is_some());
            for waiting in [Hold::Working, Hold::Quiet, Hold::NotReady] {
                let retained = retain_raised_holds(&holds, BTreeMap::from([(id.clone(), waiting)]));
                assert_eq!(
                    of(&child, Some(&ledger), &retained),
                    of(&child, Some(&ledger), &holds)
                );
            }
            assert!(retain_raised_holds(&holds, BTreeMap::new()).is_empty());
            assert!(
                of(&child, Some(&ledger), &BTreeMap::new()).is_none(),
                "a successful bell clears the hold"
            );
            for state in [State::Acknowledged, State::Cancelled, State::Expired] {
                ledger.letters[0].state = state;
                assert!(of(&child, Some(&ledger), &holds).is_none());
            }
            ledger.letters[0].state = State::Pending;
            mailbox::send(
                &mut ledger,
                &actor("parent"),
                &actor("child"),
                "reply",
                "answer",
                "reply",
                Some(id),
                200,
            )
            .unwrap();
            assert!(
                of(&child, Some(&ledger), &holds).is_none(),
                "a causal parent answer clears the request"
            );
        }
    }

    // status-model.md, Delegated escalation: a child that took up a new turn
    // after its letter moved on without the answer, so stopping again does
    // not bring the letter's raise back. The 2026-10-09 case: a block letter
    // went undelivered, the child carried on and merged its PR, and stayed
    // in Needs You after it stopped.
    #[test]
    fn session_letter_raise_ends_for_good_once_the_child_takes_up_a_later_request() {
        use crate::labels::facts::{Request, Requester};
        use crate::request_view::RowFacts;
        let request = |at, requester| Request {
            text: "next".into(),
            cut: false,
            images: 0,
            at_unix_ms: at,
            requester,
            first: false,
        };
        let cases = [
            (State::Undelivered, None, Cause::Undelivered),
            (State::Pending, Some(Hold::Blocked), Cause::ParentBlocked),
            (State::Pending, Some(Hold::Draft), Cause::Draft),
            (State::Pending, Some(Hold::Exhausted), Cause::BellExhausted),
        ];
        for (state, hold, cause) in cases {
            for requester in [Requester::Agent, Requester::Operator] {
                let mut child = child();
                let mut ledger = Ledger::default();
                mailbox::send(
                    &mut ledger,
                    &actor("child"),
                    &actor("parent"),
                    "block",
                    "question",
                    "block",
                    None,
                    100,
                )
                .unwrap();
                ledger.letters[0].state = state;
                let holds = hold
                    .map(|hold| BTreeMap::from([(ledger.letters[0].id.clone(), hold)]))
                    .unwrap_or_default();
                let facts = |at| RowFacts {
                    other_request: (requester == Requester::Agent)
                        .then(|| request(at, requester.clone())),
                    operator_request: (requester == Requester::Operator)
                        .then(|| request(at, requester.clone())),
                    ..RowFacts::default()
                };
                child.row_facts = Some(facts(90));
                assert_eq!(
                    of(&child, Some(&ledger), &holds).map(|raised| raised.cause),
                    Some(cause),
                    "the turn that wrote the letter is still the one waiting"
                );
                child.row_facts = Some(facts(100));
                assert_eq!(
                    of(&child, Some(&ledger), &holds).map(|raised| raised.cause),
                    Some(cause),
                    "a request no later than the letter is not a new turn"
                );
                child.row_facts = Some(facts(101));
                child.activity = "working".into();
                assert!(of(&child, Some(&ledger), &holds).is_none());
                child.activity = "stopped".into();
                assert!(
                    of(&child, Some(&ledger), &holds).is_none(),
                    "{cause:?} does not come back when the child stops again"
                );
                child.blocked = true;
                assert_eq!(
                    of(&child, Some(&ledger), &holds).map(|raised| raised.cause),
                    Some(Cause::ChildBlocked),
                    "the child's own menu still raises it"
                );
            }
        }
    }

    #[test]
    fn session_watch_escalates_only_the_unanswered_human_notified_current_episode() {
        let child = child();
        let mut ledger = Ledger::default();
        crate::delivery::watch::start(&mut ledger, &actor("parent"), &actor("child"), 100).unwrap();
        mailbox::send(
            &mut ledger,
            &actor("observer"),
            &actor("parent"),
            "warning",
            "warning",
            "request",
            None,
            200,
        )
        .unwrap();
        ledger.letters[0].watch_warning = Some(crate::delivery::watch::WarningReceipt {
            target: actor("child"),
            activity_at_unix_ms: 100,
            ordinal: 1,
            parent_notified: false,
        });
        assert!(of(&child, Some(&ledger), &BTreeMap::new()).is_none());
        ledger.letters[0]
            .watch_warning
            .as_mut()
            .unwrap()
            .parent_notified = true;
        let raised = of(&child, Some(&ledger), &BTreeMap::new()).unwrap();
        assert_eq!(raised.cause, Cause::ObserverUnconfirmed);
        assert!(
            raised.human_notice,
            "the existing watch notice owns the push"
        );
        ledger.watches[0].last_activity_at_unix_ms = 300;
        assert!(
            of(&child, Some(&ledger), &BTreeMap::new()).is_none(),
            "new activity ends the episode"
        );
        ledger.watches[0].last_activity_at_unix_ms = 100;
        ledger.watches.clear();
        assert!(
            of(&child, Some(&ledger), &BTreeMap::new()).is_none(),
            "cancelling the watch clears the raise"
        );
    }

    #[test]
    fn session_child_escalates_only_for_a_current_parent_hold_or_undelivered_letter() {
        let mut child = child();
        let actor = |pane: &str| Actor {
            pane_id: pane.into(),
            name: pane.into(),
            kind: "codex".into(),
            device_id: "local".into(),
            session: Some(format!("{pane}-session")),
        };
        let mut ledger = Ledger::default();
        mailbox::send(
            &mut ledger,
            &actor("child"),
            &actor("parent"),
            "request",
            "question",
            "request",
            None,
            100,
        )
        .unwrap();
        let id = ledger.letters[0].id.clone();
        assert!(of(&child, Some(&ledger), &BTreeMap::new()).is_none());
        for (hold, cause) in [
            (Hold::Blocked, Cause::ParentBlocked),
            (Hold::AwaitingOperator, Cause::ParentBlocked),
            (Hold::Draft, Cause::Draft),
            (Hold::Exhausted, Cause::BellExhausted),
        ] {
            let holds = BTreeMap::from([(id.clone(), hold)]);
            assert_eq!(of(&child, Some(&ledger), &holds).unwrap().cause, cause);
        }
        ledger.letters[0].state = State::Undelivered;
        assert!(
            of(&child, Some(&ledger), &BTreeMap::new())
                .unwrap()
                .human_notice
        );
        child.activity = "working".into();
        assert!(of(&child, Some(&ledger), &BTreeMap::new()).is_none());
        child.activity = "stopped".into();
        ledger.letters[0].recipient.session = Some("replacement-parent".into());
        assert!(of(&child, Some(&ledger), &BTreeMap::new()).is_none());
        child.blocked = true;
        assert_eq!(
            of(&child, None, &BTreeMap::new()).unwrap().cause,
            Cause::ChildBlocked
        );
    }
}
