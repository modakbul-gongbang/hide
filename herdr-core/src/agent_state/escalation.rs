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
