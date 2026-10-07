use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::answer::{Confirmed, InboxEntry};
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
    /// A prompt hook asks what to hand the agent. `bell` is true only when
    /// the submitted prompt was Hide's own bell: that turn receives the letter
    /// bodies, while any other prompt receives at most a count.
    Pull {
        #[serde(default)]
        bell: bool,
        /// The session id the hook's runtime reported. The hook only counts
        /// as a submission in the pane when it is the pane's own session.
        #[serde(default)]
        session: Option<String>,
    },
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
        #[serde(default)]
        approval: Option<String>,
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
        hook_confirmed: Some(false),
        waiting_answer: matches!(kind, "request" | "block"),
        reply_to,
        created_at_unix_ms: now,
        finished_at_unix_ms: None,
        bell_errors: 0,
        bell_sent: false,
        bell_attempts: Some(0),
        human_notified: false,
        watch_warning: None,
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
            let mut ended = None;
            if matches!(command, Command::Ack { .. }) {
                if matches!(letter.state, State::Pending | State::Delivered) {
                    letter.state = State::Acknowledged;
                    // Only the recipient's own pane and session may
                    // acknowledge, so the acknowledgement is its receipt
                    // whatever its kind; a letter already cancelled or
                    // expired has none to give.
                    ended = record_intake(letter, now);
                }
            } else {
                letter.state = State::Cancelled;
                letter.waiting_answer = false;
            }
            if !letter.open() {
                letter.finished_at_unix_ms.get_or_insert(now);
            }
            let letter = letter.clone();
            end_report_watches(ledger, ended);
            Ok(json!(letter))
        }
        Command::Show { id } => Ok(json!(authorized(ledger, actor, id)?)),
        Command::Inbox => {
            // An agent with no prompt hook acknowledges what it read.
            let acknowledges = !prompt_hook(&actor.kind);
            Ok(json!(
                ledger
                    .letters
                    .iter()
                    .filter(|letter| letter.recipient.same_identity(actor)
                        && matches!(letter.state, State::Pending | State::Undelivered)
                        || letter.sender.same_identity(actor) && letter.state == State::Undelivered)
                    .map(|letter| InboxEntry {
                        ack_command: (acknowledges
                            && letter.state == State::Pending
                            && letter.recipient.same_identity(actor))
                        .then(|| format!("hide request ack {}", letter.id)),
                        letter: letter.clone(),
                    })
                    .collect::<Vec<_>>()
            ))
        }
        Command::Pull { bell: true, .. } => Ok(json!(pull(ledger, actor)?)),
        Command::Pull { bell: false, .. } => Ok(json!(operator_prompt_intake(ledger, actor, now)?)),
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
                reports.extend(record_intake(letter, now));
            }
            // Delivery and ending the matching watch are one durable transaction.
            for report in reports {
                end_report_watches(ledger, Some(report));
            }
            Ok(json!(Confirmed {
                confirmed: ids.clone()
            }))
        }
        Command::Agents { .. }
        | Command::WatchStart { .. }
        | Command::WatchStop { .. }
        | Command::WatchAssign { .. }
        | Command::WatchList => Err("watch_command_required".into()),
    }
}

/// Records that the recipient took the letter in. A report returns the pair
/// whose watch the intake ends.
fn record_intake(letter: &mut Letter, now: u64) -> Option<(Actor, Actor)> {
    if letter.intake_confirmed() {
        return None;
    }
    letter.hook_confirmed = Some(true);
    if matches!(
        letter.state,
        State::Pending | State::Undelivered | State::Expired
    ) {
        letter.state = State::Delivered;
    }
    if !letter.waiting_answer {
        letter.finished_at_unix_ms = Some(now);
    }
    (letter.kind == "report").then(|| (letter.sender.clone(), letter.recipient.clone()))
}

fn end_report_watches(ledger: &mut Ledger, report: Option<(Actor, Actor)>) {
    if let Some((sender, recipient)) = report {
        ledger.watches.retain(|watch| {
            !(sender.same_identity(&watch.target) && recipient.same_identity(&watch.parent))
        });
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

#[derive(Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Intake {
    pub context: String,
    pub ids: Vec<String>,
    pub remaining: usize,
}

/// Whether the kind's installed prompt hook can hand letters to the agent.
/// Any other agent reads them with `hide inbox` and acknowledges them, and a
/// Claude Code hook that runs inside one (Grok, OpenCode) must not count as
/// that agent having read anything.
pub(crate) fn prompt_hook(kind: &str) -> bool {
    crate::agent_hooks::runtime_of(kind).is_some()
}

/// Whether a bell is still coming for the letter. A letter whose three bells
/// are spent, or whose recipient is never belled, stays pending for
/// `hide inbox` and expires undelivered; the operator's own prompt promises
/// it nothing.
fn bell_pending(letter: &Letter, now: u64) -> bool {
    letter.state == State::Pending
        && now.saturating_sub(letter.created_at_unix_ms) < super::DELIVERY_EXPIRY_MS
        && letter.attempts() < 3
        && super::doorbell::bell_target(&letter.recipient.kind)
}

/// What the hook hands the agent when the operator's own prompt was
/// submitted: never a letter body, only a line counting the letters a bell
/// will still bring, so the operator's turn is not mixed with them.
fn operator_prompt_intake(ledger: &Ledger, actor: &Actor, now: u64) -> Result<Intake, String> {
    actor.require_native_identity()?;
    let waiting = ledger
        .letters
        .iter()
        .filter(|letter| letter.recipient.same_identity(actor) && bell_pending(letter, now))
        .count();
    let mut intake = Intake::default();
    if waiting > 0 && prompt_hook(&actor.kind) {
        intake.context = format!("Hide 편지 {waiting}통 대기 중, 이 턴이 끝난 뒤 전달\n");
    }
    Ok(intake)
}

/// The letter bodies for the turn Hide's bell opened.
pub fn pull(ledger: &Ledger, actor: &Actor) -> Result<Intake, String> {
    actor.require_native_identity()?;
    if !prompt_hook(&actor.kind) {
        return Ok(Intake::default());
    }
    let mut pending: Vec<_> = ledger
        .letters
        .iter()
        .filter(|letter| letter.recipient.same_identity(actor) && letter.awaiting_intake())
        .collect();
    // The letters a bell rings for come first, so a backlog acknowledged
    // without a receipt never displaces the one that opened this turn.
    pending.sort_by_key(|letter| (letter.state != State::Pending, letter.created_at_unix_ms));
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
    fn pulled_letters_match_the_independent_delivery_envelope_contract() {
        let contract: serde_json::Value =
            serde_json::from_str(include_str!("../../../contracts/delivery-envelope.json"))
                .unwrap();
        let recipient = actor("recipient");
        for example in contract["examples"].as_array().unwrap() {
            let mut ledger = Ledger::default();
            let mut sender = actor("sender");
            sender.name = example["sender"].as_str().unwrap().into();
            sender.kind = example["sender_kind"].as_str().unwrap().into();
            let letter = send(
                &mut ledger,
                &sender,
                &recipient,
                "contract",
                "Please look",
                example["kind"].as_str().unwrap(),
                None,
                1,
            )
            .unwrap();
            assert_eq!(letter.id, example["id"].as_str().unwrap());
            let intake = pull(&ledger, &recipient).unwrap();
            assert_eq!(
                intake.context.trim_start().lines().next(),
                example["first_line"].as_str(),
            );
            assert_eq!(intake.ids, [letter.id]);
        }

        let mut ledger = Ledger::default();
        for (index, example) in contract["batch"]["letters"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let mut sender = actor(&format!("sender-{index}"));
            sender.name = example["sender"].as_str().unwrap().into();
            sender.kind = example["sender_kind"].as_str().unwrap().into();
            send(
                &mut ledger,
                &sender,
                &recipient,
                "contract-batch",
                example["body"].as_str().unwrap(),
                example["kind"].as_str().unwrap(),
                None,
                index as u64 + 1,
            )
            .unwrap();
        }
        assert_eq!(
            pull(&ledger, &recipient).unwrap().context,
            contract["batch"]["context"].as_str().unwrap(),
        );
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
            Command::Pull {
                bell: true,
                session: None,
            },
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

    fn pending_for(ledger: &mut Ledger, recipient: &Actor, count: usize) -> Vec<String> {
        (0..count)
            .map(|index| {
                send(
                    ledger,
                    &actor(&format!("sender-{index}")),
                    recipient,
                    &format!("intent-{index}"),
                    &format!("body-{index}"),
                    "request",
                    None,
                    index as u64 + 1,
                )
                .unwrap()
                .id
            })
            .collect()
    }

    fn pull_for(ledger: &mut Ledger, recipient: &Actor, bell: bool) -> Intake {
        let answer = apply(
            ledger,
            recipient,
            None,
            &Command::Pull {
                bell,
                session: None,
            },
            100,
        )
        .unwrap();
        serde_json::from_value(answer).unwrap()
    }

    #[test]
    fn an_operator_prompt_gets_a_count_while_a_bell_is_coming_and_the_bell_gets_the_bodies() {
        let mut ledger = Ledger::default();
        let recipient = actor("recipient");
        let ids = pending_for(&mut ledger, &recipient, 2);
        let operator = pull_for(&mut ledger, &recipient, false);
        assert!(operator.ids.is_empty(), "nothing to confirm");
        assert_eq!(
            operator.context.trim(),
            "Hide 편지 2통 대기 중, 이 턴이 끝난 뒤 전달"
        );
        assert!(!operator.context.contains("body-"));
        let bell = pull_for(&mut ledger, &recipient, true);
        assert_eq!(bell.ids, ids);
        assert!(bell.context.contains("body-0") && bell.context.contains("body-1"));
        assert!(!bell.context.contains("대기 중"));
        // Neither pull confirms anything.
        assert_eq!(
            ledger
                .letters
                .iter()
                .filter(|l| l.intake_confirmed())
                .count(),
            0
        );
    }

    #[test]
    fn the_operators_prompt_never_carries_a_body_and_counts_only_letters_a_bell_will_bring() {
        let mut ledger = Ledger::default();
        let recipient = actor("recipient");
        pending_for(&mut ledger, &recipient, 3);
        // Three bells spent: the letter waits for `hide inbox` and expires.
        ledger.letters[0].bell_attempts = Some(3);
        // Acknowledged before intake: no bell either.
        ledger.letters[1].state = State::Acknowledged;
        ledger.letters[1].hook_confirmed = Some(false);
        let operator = pull_for(&mut ledger, &recipient, false);
        assert!(operator.ids.is_empty());
        assert_eq!(
            operator.context.trim(),
            "Hide 편지 1통 대기 중, 이 턴이 끝난 뒤 전달"
        );
        ledger.letters[2].bell_attempts = Some(3);
        let operator = pull_for(&mut ledger, &recipient, false);
        assert!(operator.ids.is_empty() && operator.context.is_empty());
    }

    #[test]
    fn a_prompt_hook_that_runs_inside_an_agent_without_one_hands_over_nothing() {
        for kind in ["grok", "opencode", "gemini", "cursor"] {
            let mut ledger = Ledger::default();
            let mut recipient = actor("recipient");
            recipient.kind = kind.into();
            pending_for(&mut ledger, &recipient, 1);
            for bell in [false, true] {
                let intake = pull_for(&mut ledger, &recipient, bell);
                assert!(intake.ids.is_empty() && intake.context.is_empty(), "{kind}");
            }
        }
    }

    #[test]
    fn acknowledging_is_the_receipt_for_every_agent_kind() {
        for kind in ["grok", "gemini", "codex", "claude"] {
            let mut ledger = Ledger::default();
            let child = actor("child");
            let mut parent = actor("parent");
            parent.kind = kind.into();
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
            let acknowledged = apply(
                &mut ledger,
                &parent,
                None,
                &Command::Ack { id: report.id },
                3,
            )
            .unwrap();
            assert_eq!(acknowledged["state"], "acknowledged", "{kind}");
            assert_eq!(acknowledged["hook_confirmed"], true, "{kind}");
            assert!(!ledger.letters[0].open(), "{kind}");
            assert!(ledger.watches.is_empty(), "{kind}");
            assert!(pull(&ledger, &parent).unwrap().ids.is_empty(), "{kind}");
        }
    }

    #[test]
    fn a_pull_from_an_older_kit_is_an_operator_prompt_pull() {
        assert_eq!(
            serde_json::from_str::<Command>(r#"{"op":"pull"}"#).unwrap(),
            Command::Pull {
                bell: false,
                session: None
            }
        );
    }

    #[test]
    fn the_inbox_shows_an_agent_without_a_prompt_hook_the_command_that_acknowledges() {
        let mut ledger = Ledger::default();
        let mut recipient = actor("recipient");
        recipient.kind = "grok".into();
        let ids = pending_for(&mut ledger, &recipient, 1);
        let inbox = apply(&mut ledger, &recipient, None, &Command::Inbox, 5).unwrap();
        assert_eq!(
            inbox[0]["ack_command"],
            format!("hide request ack {}", ids[0])
        );
        // An expired letter can no longer be acknowledged, so none is offered.
        assert!(ledger.expire(10 + super::super::DELIVERY_EXPIRY_MS));
        let inbox = apply(&mut ledger, &recipient, None, &Command::Inbox, 5).unwrap();
        assert_eq!(inbox[0]["state"], "undelivered");
        assert!(inbox[0].get("ack_command").is_none());
        // A hooked agent's prompt hook acknowledges for it; the sender sees none.
        let hooked = actor("hooked");
        pending_for(&mut ledger, &hooked, 1);
        let inbox = apply(&mut ledger, &hooked, None, &Command::Inbox, 5).unwrap();
        assert!(inbox[0].get("ack_command").is_none());
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
    fn canceling_a_report_never_ends_a_watch() {
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
        apply(
            &mut ledger,
            &child,
            None,
            &Command::Cancel { id: report.id },
            3,
        )
        .unwrap();
        assert_eq!(ledger.watches.len(), 1);
    }

    #[test]
    fn ack_between_pull_and_confirmation_ends_only_matching_watch_once_across_restart() {
        let mut ledger = Ledger::default();
        let child = actor("child");
        let parent = actor("parent");
        let other = actor("other");
        super::super::watch::start(&mut ledger, &parent, &child, 1).unwrap();
        let unrelated = super::super::watch::start(&mut ledger, &other, &child, 1).unwrap();
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
        let intake = pull(&ledger, &parent).unwrap();
        assert_eq!(intake.ids.as_slice(), std::slice::from_ref(&report.id));
        // An interruption after Pull leaves the same report and both watches.
        let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
        assert_eq!(pull(&restored, &parent).unwrap().ids, intake.ids);
        assert_eq!(restored.watches.len(), 2);
        let acknowledged = apply(
            &mut restored,
            &parent,
            None,
            &Command::Ack {
                id: report.id.clone(),
            },
            3,
        )
        .unwrap();
        assert_eq!(acknowledged["state"], "acknowledged");
        assert_eq!(acknowledged["hook_confirmed"], true);
        assert_eq!(restored.watches.len(), 1);
        assert_eq!(restored.watches[0].id, unrelated.id);
        assert!(pull(&restored, &parent).unwrap().ids.is_empty());
        // The interrupted hook's confirmation still answers and changes nothing.
        let mut restored: Ledger = serde_json::from_slice(&restored.bytes().unwrap()).unwrap();
        let rearmed = super::super::watch::start(&mut restored, &parent, &child, 4).unwrap();
        let before = restored.clone();
        let confirm = Command::Confirm { ids: intake.ids };
        assert_eq!(
            apply(&mut restored, &parent, None, &confirm, 5).unwrap()["confirmed"],
            json!([report.id.clone()])
        );
        assert_eq!(restored, before);
        assert!(restored.watches.iter().any(|watch| watch.id == rearmed.id));
        assert_eq!(
            send(
                &mut restored,
                &child,
                &parent,
                "report",
                "different",
                "report",
                None,
                7
            )
            .unwrap()
            .id,
            report.id
        );
        assert_eq!(restored.letters.len(), 1);
    }

    #[test]
    fn confirmation_then_ack_preserves_rearmed_watch_on_replay_after_restart() {
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
        let confirm = Command::Confirm {
            ids: pull(&ledger, &parent).unwrap().ids,
        };
        apply(&mut ledger, &parent, None, &confirm, 3).unwrap();
        assert!(ledger.watches.is_empty());
        let acknowledged = apply(
            &mut ledger,
            &parent,
            None,
            &Command::Ack { id: report.id },
            4,
        )
        .unwrap();
        assert_eq!(acknowledged["state"], "acknowledged");
        assert_eq!(acknowledged["hook_confirmed"], true);
        let rearmed = super::super::watch::start(&mut ledger, &parent, &child, 5).unwrap();
        let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
        let before = restored.clone();
        apply(&mut restored, &parent, None, &confirm, 6).unwrap();
        assert_eq!(restored, before);
        assert_eq!(restored.watches[0].id, rearmed.id);
    }

    #[test]
    fn an_acknowledgement_after_an_interrupted_pull_is_the_receipt() {
        for kind in ["request", "report"] {
            let mut ledger = Ledger::default();
            let sender = actor("sender");
            let recipient = actor("recipient");
            let letter = send(
                &mut ledger,
                &sender,
                &recipient,
                "once",
                "body",
                kind,
                None,
                1,
            )
            .unwrap();
            let intake = pull(&ledger, &recipient).unwrap();
            assert_eq!(intake.ids.as_slice(), std::slice::from_ref(&letter.id));
            apply(
                &mut ledger,
                &recipient,
                None,
                &Command::Ack { id: letter.id },
                2,
            )
            .unwrap();
            let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
            assert!(pull(&restored, &recipient).unwrap().ids.is_empty());
            let before = restored.clone();
            apply(
                &mut restored,
                &recipient,
                None,
                &Command::Confirm { ids: intake.ids },
                3,
            )
            .unwrap();
            assert_eq!(restored, before);
        }
    }

    #[test]
    fn actual_confirmation_after_cancel_or_deadline_ends_watch_once_and_survives_restart() {
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
            let confirm = Command::Confirm {
                ids: pull(&ledger, &parent).unwrap().ids,
            };
            let now = 2 + super::super::DELIVERY_EXPIRY_MS;
            if cancel {
                assert_eq!(
                    apply(
                        &mut ledger,
                        &child,
                        None,
                        &Command::Cancel {
                            id: report.id.clone()
                        },
                        now
                    )
                    .unwrap()["state"],
                    "cancelled"
                );
            } else {
                assert!(ledger.expire(now));
                assert_eq!(
                    apply(
                        &mut ledger,
                        &parent,
                        None,
                        &Command::Show {
                            id: report.id.clone()
                        },
                        now
                    )
                    .unwrap()["state"],
                    "undelivered"
                );
            }
            assert_eq!(ledger.watches.len(), 1);
            assert!(pull(&ledger, &parent).unwrap().ids.is_empty());
            let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
            assert_eq!(
                apply(&mut restored, &parent, None, &confirm, now + 1).unwrap()["confirmed"],
                json!([report.id.clone()])
            );
            assert!(restored.watches.is_empty());
            let shown = apply(
                &mut restored,
                &parent,
                None,
                &Command::Show { id: report.id },
                now + 1,
            )
            .unwrap();
            assert_eq!(
                shown["state"],
                if cancel { "cancelled" } else { "delivered" }
            );
            assert_eq!(shown["hook_confirmed"], true);
            let rearmed =
                super::super::watch::start(&mut restored, &parent, &child, now + 2).unwrap();
            let mut restored: Ledger = serde_json::from_slice(&restored.bytes().unwrap()).unwrap();
            let before = restored.clone();
            apply(&mut restored, &parent, None, &confirm, now + 3).unwrap();
            assert_eq!(restored, before);
            assert_eq!(restored.watches[0].id, rearmed.id);
        }
    }

    /// A letter a build before this one acknowledged without a receipt.
    fn acknowledged_without_receipt(ledger: &mut Ledger, index: usize) {
        let letter = &mut ledger.letters[index];
        letter.state = State::Acknowledged;
        letter.hook_confirmed = Some(false);
        letter.finished_at_unix_ms = None;
    }

    #[test]
    fn a_bell_turn_hands_over_pending_letters_before_an_acknowledged_backlog() {
        let mut ledger = Ledger::default();
        let recipient = actor("recipient");
        pending_for(&mut ledger, &recipient, HOOK_LETTERS + 2);
        for index in 0..HOOK_LETTERS + 1 {
            acknowledged_without_receipt(&mut ledger, index);
        }
        let fresh = ledger.letters[HOOK_LETTERS + 1].id.clone();
        let intake = pull_for(&mut ledger, &recipient, true);
        assert_eq!(intake.ids.len(), HOOK_LETTERS);
        assert_eq!(intake.ids[0], fresh, "the letter the bell rang for");
        assert_eq!(
            intake.ids[1], ledger.letters[0].id,
            "then the oldest backlog"
        );
        assert_eq!(intake.remaining, 2);
    }

    #[test]
    fn an_acknowledged_letter_without_a_receipt_stops_awaiting_intake_at_its_deadline() {
        let mut ledger = Ledger::default();
        let sender = actor("sender");
        let recipient = actor("recipient");
        for index in 0..OPEN_LIMIT {
            let kind = if index == 0 { "block" } else { "report" };
            send(
                &mut ledger,
                &sender,
                &recipient,
                &format!("receipt-{index}"),
                "body",
                kind,
                None,
                1,
            )
            .unwrap();
            acknowledged_without_receipt(&mut ledger, index);
        }
        let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
        let deadline = 1 + super::super::DELIVERY_EXPIRY_MS;
        assert!(!restored.expire(deadline - 1));
        assert_eq!(
            send(
                &mut restored,
                &sender,
                &recipient,
                "overflow",
                "body",
                "report",
                None,
                deadline - 1
            )
            .unwrap_err(),
            "capacity"
        );
        assert_eq!(pull(&restored, &recipient).unwrap().ids.len(), HOOK_LETTERS);
        assert!(restored.expire(deadline));
        assert!(pull(&restored, &recipient).unwrap().ids.is_empty());
        // A block still awaiting its reply stays open; the reports do not.
        assert_eq!(restored.letters.iter().filter(|l| l.open()).count(), 1);
        let report = restored.letters[1].id.clone();
        let shown = apply(
            &mut restored,
            &recipient,
            None,
            &Command::Show { id: report },
            deadline,
        )
        .unwrap();
        assert_eq!(shown["state"], "acknowledged");
        assert!(shown["hook_confirmed"].is_null());
        assert_eq!(shown["finished_at_unix_ms"], deadline);
        send(
            &mut restored,
            &sender,
            &recipient,
            "overflow",
            "body",
            "report",
            None,
            deadline,
        )
        .unwrap();
        let reloaded: Ledger = serde_json::from_slice(&restored.bytes().unwrap()).unwrap();
        assert_eq!(reloaded, restored);
        assert!(!restored.clone().expire(deadline + 1));
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
