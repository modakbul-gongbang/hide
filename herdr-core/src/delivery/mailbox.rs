use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::ledger::{Ledger, Letter, State};
use super::{Actor, BODY_LIMIT, HOOK_LETTERS, HOOK_LIMIT, LETTER_LIMIT, OPEN_LIMIT, RETENTION_MS};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Agents {
        command: crate::coordination::Command,
    },
    Send {
        target: String,
        intent: String,
        body: String,
        #[serde(default = "request_kind")]
        kind: String,
    },
    Reply {
        id: String,
        intent: String,
        body: String,
    },
    Ack {
        id: String,
    },
    Cancel {
        id: String,
    },
    Show {
        id: String,
    },
    Inbox,
    Pull,
    Confirm {
        ids: Vec<String>,
    },
    WatchStart {
        target: String,
        observer: Option<String>,
        actor: Option<String>,
    },
    WatchStop {
        id: String,
    },
    WatchAssign {
        id: String,
        observer: String,
        actor: Option<String>,
        expected_generation: Option<u64>,
    },
    WatchList,
}

fn request_kind() -> String {
    "request".into()
}

pub(crate) fn existing_intent<'a>(
    ledger: &'a Ledger,
    sender: &Actor,
    intent: &str,
    now: u64,
) -> Option<&'a Letter> {
    ledger.letters.iter().find(|letter| {
        letter.sender.same_identity(sender)
            && letter.intent == intent
            && (letter.open() || now.saturating_sub(letter.created_at_unix_ms) < RETENTION_MS)
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "The private transaction validates each immutable envelope field before persistence"
)]
pub(crate) fn send(
    ledger: &mut Ledger,
    sender: &Actor,
    recipient: &Actor,
    intent: &str,
    body: &str,
    kind: &str,
    reply_to: Option<String>,
    now: u64,
) -> Result<Letter, String> {
    sender.require_native_identity()?;
    recipient.require_native_identity()?;
    if !sender.valid() || !recipient.valid() {
        return Err("invalid_actor".into());
    }
    if !super::valid_key(intent) {
        return Err("invalid_intent".into());
    }
    if !matches!(kind, "request" | "block" | "report" | "reply" | "watch") {
        return Err("invalid_kind".into());
    }
    if let Some(letter) = existing_intent(ledger, sender, intent, now) {
        return Ok(letter.clone());
    }
    if body.len() > BODY_LIMIT
        || ledger.letters.len() >= LETTER_LIMIT
        || ledger.letters.iter().filter(|letter| letter.open()).count() >= OPEN_LIMIT
    {
        return Err("capacity".into());
    }
    if body.trim().is_empty() {
        return Err("body_required".into());
    }
    let sequence = ledger.next_id;
    ledger.next_id = sequence.checked_add(1).ok_or("capacity")?;
    let letter = Letter {
        id: format!("letter-{sequence}"),
        intent: intent.to_owned(),
        sender: sender.clone(),
        recipient: recipient.clone(),
        kind: kind.to_owned(),
        body: body.to_owned(),
        state: State::Pending,
        waiting_answer: matches!(kind, "request" | "block"),
        reply_to,
        created_at_unix_ms: now,
        finished_at_unix_ms: None,
        bell_errors: 0,
        bell_sent: false,
        bell_attempts: Some(0),
        human_notified: false,
    };
    ledger.letters.push(letter.clone());
    Ok(letter)
}

pub fn apply(
    ledger: &mut Ledger,
    actor: &Actor,
    target: Option<&Actor>,
    command: &Command,
    now: u64,
) -> Result<Value, String> {
    actor.require_native_identity()?;
    if let Command::Send { intent, .. } | Command::Reply { intent, .. } = command
        && let Some(letter) = existing_intent(ledger, actor, intent, now)
    {
        return Ok(json!(letter));
    }
    match command {
        Command::Send {
            intent, body, kind, ..
        } => Ok(json!(send(
            ledger,
            actor,
            target.ok_or("target_unavailable")?,
            intent,
            body,
            kind,
            None,
            now
        )?)),
        Command::Reply { id, intent, body } => {
            let original = authorized(ledger, actor, id)?.clone();
            if !original.recipient.same_identity(actor) {
                return Err("recipient_required".into());
            }
            let reply = send(
                ledger,
                actor,
                &original.sender,
                intent,
                body,
                "reply",
                Some(id.clone()),
                now,
            )?;
            let original = ledger
                .letters
                .iter_mut()
                .find(|letter| letter.id == *id)
                .ok_or("letter_unavailable")?;
            original.waiting_answer = false;
            if original.state != State::Pending {
                original.finished_at_unix_ms.get_or_insert(now);
            }
            Ok(json!(reply))
        }
        Command::Ack { id } | Command::Cancel { id } => {
            let original = authorized(ledger, actor, id)?;
            if matches!(command, Command::Ack { .. }) && !original.recipient.same_identity(actor) {
                return Err("recipient_required".into());
            }
            if matches!(command, Command::Cancel { .. }) && !original.sender.same_identity(actor) {
                return Err("sender_required".into());
            }
            let letter = ledger
                .letters
                .iter_mut()
                .find(|letter| letter.id == *id)
                .ok_or("letter_unavailable")?;
            if matches!(command, Command::Ack { .. }) {
                if matches!(letter.state, State::Pending | State::Delivered) {
                    letter.state = State::Acknowledged;
                }
            } else {
                letter.state = State::Cancelled;
                letter.waiting_answer = false;
            }
            if !letter.open() {
                letter.finished_at_unix_ms.get_or_insert(now);
            }
            Ok(json!(letter))
        }
        Command::Show { id } => Ok(json!(authorized(ledger, actor, id)?)),
        Command::Inbox => Ok(json!(
            ledger
                .letters
                .iter()
                .filter(|letter| letter.recipient.same_identity(actor)
                    && matches!(letter.state, State::Pending | State::Undelivered)
                    || letter.sender.same_identity(actor) && letter.state == State::Undelivered)
                .collect::<Vec<_>>()
        )),
        Command::Pull => Ok(json!(pull(ledger, actor)?)),
        Command::Confirm { ids } => {
            if ids.len() > HOOK_LETTERS {
                return Err("capacity".into());
            }
            for id in ids {
                let letter = authorized(ledger, actor, id)?;
                if !letter.recipient.same_identity(actor) {
                    return Err("recipient_required".into());
                }
            }
            let mut reports = Vec::new();
            for id in ids {
                let letter = ledger
                    .letters
                    .iter_mut()
                    .find(|letter| letter.id == *id)
                    .ok_or("letter_unavailable")?;
                if letter.state == State::Pending {
                    if letter.kind == "report" {
                        reports.push((letter.sender.clone(), letter.recipient.clone()));
                    }
                    letter.state = State::Delivered;
                    if !letter.waiting_answer {
                        letter.finished_at_unix_ms = Some(now);
                    }
                }
            }
            // Delivery and ending the matching watch are one durable transaction.
            ledger.watches.retain(|watch| {
                !reports.iter().any(|(sender, recipient)| {
                    sender.same_identity(&watch.target) && recipient.same_identity(&watch.parent)
                })
            });
            Ok(json!({"confirmed":ids}))
        }
        Command::Agents { .. }
        | Command::WatchStart { .. }
        | Command::WatchStop { .. }
        | Command::WatchAssign { .. }
        | Command::WatchList => Err("watch_command_required".into()),
    }
}

fn authorized<'a>(ledger: &'a Ledger, actor: &Actor, id: &str) -> Result<&'a Letter, String> {
    ledger
        .letters
        .iter()
        .find(|letter| {
            letter.id == id
                && (letter.sender.same_identity(actor) || letter.recipient.same_identity(actor))
        })
        .ok_or_else(|| "letter_unavailable".into())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Intake {
    pub context: String,
    pub ids: Vec<String>,
    pub remaining: usize,
}

pub fn pull(ledger: &Ledger, actor: &Actor) -> Result<Intake, String> {
    actor.require_native_identity()?;
    let mut pending: Vec<_> = ledger
        .letters
        .iter()
        .filter(|letter| letter.recipient.same_identity(actor) && letter.state == State::Pending)
        .collect();
    pending.sort_by_key(|letter| letter.created_at_unix_ms);
    let mut context = String::new();
    let mut ids = Vec::new();
    // Reserve the tail before rendering bodies, including IDs and sender
    // metadata. Truncation always ends at a UTF-8 boundary.
    const TAIL_BUDGET: usize = 128;
    for letter in pending.iter().take(HOOK_LETTERS) {
        let header = format!(
            "\nHide letter {} from {} ({}) [{}]\n",
            letter.id, letter.sender.name, letter.sender.kind, letter.kind
        );
        let tail = format!("\nFull letter: hide request show {}\n", letter.id);
        let available =
            HOOK_LIMIT.saturating_sub(context.len() + header.len() + tail.len() + TAIL_BUDGET);
        if available == 0 {
            break;
        }
        let mut end = letter.body.len().min(available);
        while !letter.body.is_char_boundary(end) {
            end -= 1;
        }
        context.push_str(&header);
        context.push_str(&letter.body[..end]);
        context.push_str(&tail);
        ids.push(letter.id.clone());
    }
    let remaining = pending.len().saturating_sub(ids.len());
    if remaining > 0 {
        context.push_str(&format!(
            "\n대기 {remaining}통 더 있음. hide inbox로 확인하세요.\n"
        ));
    }
    Ok(Intake {
        context,
        ids,
        remaining,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn actor(id: &str) -> Actor {
        Actor {
            pane_id: id.into(),
            name: id.into(),
            kind: "codex".into(),
            device_id: "local".into(),
            session: Some(format!("session-{id}")),
        }
    }

    #[test]
    fn missing_native_identity_cannot_send_read_or_confirm_previous_occupant_mail() {
        let mut ledger = Ledger::default();
        let sender = actor("sender");
        let recipient = actor("recipient");
        let letter = send(
            &mut ledger,
            &sender,
            &recipient,
            "first",
            "private",
            "request",
            None,
            1,
        )
        .unwrap();
        let mut unknown = recipient.clone();
        unknown.session = None;
        assert_eq!(
            send(
                &mut ledger,
                &sender,
                &unknown,
                "second",
                "private",
                "request",
                None,
                2
            )
            .unwrap_err(),
            "native_identity_required"
        );
        // A retained legacy record with no native owner must not turn two
        // missing references in the same pane into authorization.
        ledger.letters[0].recipient.session = None;
        let before = ledger.clone();
        for command in [
            Command::Inbox,
            Command::Pull,
            Command::Show {
                id: letter.id.clone(),
            },
            Command::Confirm {
                ids: vec![letter.id.clone()],
            },
            Command::Send {
                target: sender.pane_id.clone(),
                intent: "third".into(),
                body: "private".into(),
                kind: "request".into(),
            },
        ] {
            assert_eq!(
                apply(&mut ledger, &unknown, Some(&sender), &command, 2).unwrap_err(),
                "native_identity_required"
            );
            assert_eq!(ledger, before);
        }
        assert_eq!(
            pull(&ledger, &unknown).unwrap_err(),
            "native_identity_required"
        );
        let mut replacement = recipient.clone();
        replacement.session = Some("new-occupant".into());
        assert!(pull(&ledger, &replacement).unwrap().ids.is_empty());
        for command in [
            Command::Show {
                id: letter.id.clone(),
            },
            Command::Confirm {
                ids: vec![letter.id],
            },
        ] {
            assert_eq!(
                apply(&mut ledger, &replacement, None, &command, 2).unwrap_err(),
                "letter_unavailable"
            );
            assert_eq!(ledger, before);
        }
    }
    #[test]
    fn same_intent_converges_across_confirmation_cancel_and_restart() {
        let mut ledger = Ledger::default();
        let a = actor("a");
        let b = actor("b");
        let letter = send(&mut ledger, &a, &b, "intent", "body", "request", None, 1).unwrap();
        assert_eq!(pull(&ledger, &b).unwrap().ids, vec![letter.id.clone()]);
        assert_eq!(pull(&ledger, &b).unwrap().ids, vec![letter.id.clone()]);
        apply(
            &mut ledger,
            &b,
            None,
            &Command::Confirm {
                ids: vec![letter.id.clone()],
            },
            2,
        )
        .unwrap();
        assert!(pull(&ledger, &b).unwrap().ids.is_empty());
        apply(
            &mut ledger,
            &a,
            None,
            &Command::Cancel {
                id: letter.id.clone(),
            },
            3,
        )
        .unwrap();
        let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
        assert_eq!(
            send(
                &mut restored,
                &a,
                &b,
                "intent",
                "different",
                "request",
                None,
                4
            )
            .unwrap()
            .id,
            letter.id
        );
        assert_eq!(restored.letters.len(), 1);
        assert_eq!(restored.letters[0].state, State::Cancelled);
    }
    #[test]
    fn intake_is_oldest_five_and_bounded_utf8_without_losing_full_body() {
        let mut ledger = Ledger::default();
        let a = actor("a");
        let b = actor("b");
        for index in 0..7 {
            send(
                &mut ledger,
                &a,
                &b,
                &format!("i{index}"),
                "body",
                "request",
                None,
                index,
            )
            .unwrap();
        }
        let intake = pull(&ledger, &b).unwrap();
        assert_eq!(intake.ids.len(), 5);
        assert_eq!(intake.remaining, 2);
        assert!(intake.context.contains("대기 2통 더 있음"));
        ledger.letters[0].body = "한".repeat(BODY_LIMIT / 3);
        let intake = pull(&ledger, &b).unwrap();
        assert!(intake.context.len() <= HOOK_LIMIT);
        assert!(intake.context.contains("hide request show letter-1"));
        assert_eq!(ledger.letters[0].body.len(), (BODY_LIMIT / 3) * 3);
    }
    #[test]
    fn expiry_and_capacity_are_explicit_without_eviction() {
        let mut ledger = Ledger::default();
        let a = actor("a");
        let b = actor("b");
        let letter = send(&mut ledger, &a, &b, "i", "body", "request", None, 10).unwrap();
        assert!(!ledger.expire(10 + super::super::DELIVERY_EXPIRY_MS - 1));
        assert!(ledger.expire(10 + super::super::DELIVERY_EXPIRY_MS));
        assert_eq!(ledger.letters[0].state, State::Undelivered);
        assert_eq!(
            send(
                &mut ledger,
                &a,
                &b,
                "big",
                &"x".repeat(BODY_LIMIT + 1),
                "request",
                None,
                20
            )
            .unwrap_err(),
            "capacity"
        );
        assert_eq!(ledger.letters[0].id, letter.id);
        let inbox = apply(&mut ledger, &a, None, &Command::Inbox, 20).unwrap();
        assert_eq!(inbox[0]["state"], "undelivered");
    }

    #[test]
    fn repeated_intent_does_not_require_a_live_target_or_close_another_request() {
        let mut ledger = Ledger::default();
        let a = actor("a");
        let b = actor("b");
        let first = send(&mut ledger, &a, &b, "first", "work", "request", None, 1).unwrap();
        let retry = apply(
            &mut ledger,
            &a,
            None,
            &Command::Send {
                target: "gone".into(),
                intent: "first".into(),
                body: "changed".into(),
                kind: "request".into(),
            },
            2,
        )
        .unwrap();
        assert_eq!(retry["id"], first.id);
        let second = send(&mut ledger, &a, &b, "second", "other", "request", None, 3).unwrap();
        let reply = apply(
            &mut ledger,
            &b,
            None,
            &Command::Reply {
                id: first.id,
                intent: "reply".into(),
                body: "done".into(),
            },
            4,
        )
        .unwrap();
        let retry = apply(
            &mut ledger,
            &b,
            None,
            &Command::Reply {
                id: second.id.clone(),
                intent: "reply".into(),
                body: "different decision".into(),
            },
            5,
        )
        .unwrap();
        assert_eq!(retry["id"], reply["id"]);
        assert!(
            ledger
                .letters
                .iter()
                .find(|letter| letter.id == second.id)
                .unwrap()
                .waiting_answer
        );
        assert_eq!(ledger.letters.len(), 3);
    }

    #[test]
    fn report_ends_only_its_parent_watch_on_hook_confirmation_and_replays_once() {
        let mut ledger = Ledger::default();
        let child = actor("child");
        let parent = actor("parent");
        let other = actor("other");
        super::super::watch::start(&mut ledger, &parent, &child, 1).unwrap();
        super::super::watch::start(&mut ledger, &other, &child, 1).unwrap();
        let report = send(
            &mut ledger,
            &child,
            &parent,
            "report",
            "done",
            "report",
            None,
            2,
        )
        .unwrap();
        assert!(!report.waiting_answer);
        assert_eq!(pull(&ledger, &parent).unwrap().ids, vec![report.id.clone()]);
        assert_eq!(ledger.watches.len(), 2);
        let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
        assert_eq!(
            send(
                &mut restored,
                &child,
                &parent,
                "report",
                "changed",
                "report",
                None,
                3
            )
            .unwrap()
            .id,
            report.id
        );
        apply(
            &mut restored,
            &parent,
            None,
            &Command::Confirm {
                ids: vec![report.id.clone()],
            },
            4,
        )
        .unwrap();
        assert_eq!(restored.watches.len(), 1);
        assert_eq!(restored.watches[0].parent, other);
        assert_eq!(restored.letters[0].state, State::Delivered);
        assert!(pull(&restored, &parent).unwrap().ids.is_empty());
        // Starting another episode after delivery is explicitly supported.
        super::super::watch::start(&mut restored, &parent, &child, 5).unwrap();
        apply(
            &mut restored,
            &parent,
            None,
            &Command::Confirm {
                ids: vec![report.id],
            },
            6,
        )
        .unwrap();
        assert_eq!(restored.watches.len(), 2);
    }

    #[test]
    fn acknowledging_or_canceling_a_report_never_ends_a_watch() {
        for cancel in [false, true] {
            let mut ledger = Ledger::default();
            let child = actor("child");
            let parent = actor("parent");
            super::super::watch::start(&mut ledger, &parent, &child, 1).unwrap();
            let report = send(
                &mut ledger,
                &child,
                &parent,
                "report",
                "done",
                "report",
                None,
                2,
            )
            .unwrap();
            let command = if cancel {
                Command::Cancel { id: report.id }
            } else {
                Command::Ack { id: report.id }
            };
            apply(
                &mut ledger,
                if cancel { &child } else { &parent },
                None,
                &command,
                3,
            )
            .unwrap();
            assert_eq!(ledger.watches.len(), 1);
        }
    }

    #[test]
    fn remote_letters_use_the_same_durable_intent_and_native_session_boundary() {
        let mut ledger = Ledger::default();
        let local = actor("local");
        let mut remote = actor("remote:device:pane:target");
        remote.device_id = "device".into();
        let letter = send(
            &mut ledger,
            &local,
            &remote,
            "remote",
            "body",
            "block",
            None,
            1,
        )
        .unwrap();
        assert!(letter.waiting_answer);
        let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
        assert_eq!(
            pull(&restored, &remote).unwrap().ids,
            vec![letter.id.clone()]
        );
        let mut replaced = remote.clone();
        replaced.session = Some("replacement".into());
        assert!(pull(&restored, &replaced).unwrap().ids.is_empty());
        apply(
            &mut restored,
            &remote,
            None,
            &Command::Confirm {
                ids: vec![letter.id.clone()],
            },
            2,
        )
        .unwrap();
        assert!(pull(&restored, &remote).unwrap().ids.is_empty());
        assert_eq!(
            send(
                &mut restored,
                &local,
                &remote,
                "remote",
                "changed",
                "report",
                None,
                3
            )
            .unwrap()
            .id,
            letter.id
        );
    }

    #[test]
    fn open_and_retained_limits_reject_without_removing_existing_letters() {
        let mut ledger = Ledger::default();
        let a = actor("a");
        let b = actor("b");
        for index in 0..OPEN_LIMIT {
            send(
                &mut ledger,
                &a,
                &b,
                &format!("open-{index}"),
                "body",
                "request",
                None,
                1,
            )
            .unwrap();
        }
        assert_eq!(
            send(&mut ledger, &a, &b, "overflow", "body", "request", None, 1).unwrap_err(),
            "capacity"
        );
        assert_eq!(ledger.letters.len(), OPEN_LIMIT);
        assert!(!ledger.cleanup(RETENTION_MS * 2));
        for letter in &mut ledger.letters {
            letter.state = State::Cancelled;
            letter.waiting_answer = false;
            letter.finished_at_unix_ms = Some(1);
        }
        for index in OPEN_LIMIT..LETTER_LIMIT {
            let letter = send(
                &mut ledger,
                &a,
                &b,
                &format!("closed-{index}"),
                "body",
                "reply",
                None,
                1,
            )
            .unwrap();
            apply(&mut ledger, &a, None, &Command::Cancel { id: letter.id }, 1).unwrap();
        }
        assert_eq!(
            send(
                &mut ledger,
                &a,
                &b,
                "retained-overflow",
                "body",
                "reply",
                None,
                1
            )
            .unwrap_err(),
            "capacity"
        );
        assert_eq!(ledger.letters.len(), LETTER_LIMIT);
        assert!(!ledger.cleanup(RETENTION_MS));
        assert!(ledger.cleanup(RETENTION_MS + 1));
        assert!(ledger.letters.is_empty());
    }
}
