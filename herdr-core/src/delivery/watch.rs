use serde::{Deserialize, Serialize};

use super::ledger::Ledger;
use super::{Actor, FILE_LIMIT, INACTIVITY_MS, SECOND_WARNING_MS, WATCH_LIMIT};

/// The inactivity episode and external notification reservation survive the
/// watch itself, so stopping/restarting or sibling watches cannot resend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarningReceipt {
    pub target: Actor,
    pub activity_at_unix_ms: u64,
    pub ordinal: u8,
    #[serde(default)]
    pub parent_notified: bool,
}

impl WarningReceipt {
    pub(crate) fn valid(&self) -> bool {
        self.target.valid() && matches!(self.ordinal, 1 | 2)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watch {
    pub id: String,
    pub parent: Actor,
    pub target: Actor,
    pub last_activity_at_unix_ms: u64,
    pub first_warning_at_unix_ms: Option<u64>,
    pub warning_count: u8,
    pub activity_failures: u32,
    pub last_status: String,
    pub last_state_change_seq: Option<u64>,
    pub status_changed_at_unix_ms: u64,
    #[serde(default)]
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct View {
    pub id: String,
    pub parent_pane_id: String,
    pub target_pane_id: String,
    pub warning_count: u8,
    pub first_warning_at_unix_ms: Option<u64>,
    pub last_activity_at_unix_ms: u64,
}

impl Watch {
    pub fn view(&self) -> View {
        View {
            id: self.id.clone(),
            parent_pane_id: self.parent.pane_id.clone(),
            target_pane_id: self.target.pane_id.clone(),
            warning_count: self.warning_count,
            first_warning_at_unix_ms: self.first_warning_at_unix_ms,
            last_activity_at_unix_ms: self.last_activity_at_unix_ms,
        }
    }

    pub fn valid(&self) -> bool {
        super::valid_key(&self.id)
            && self.parent.valid()
            && self.target.valid()
            && self.warning_count <= 2
            && super::valid_key(&self.last_status)
            && (self.warning_count == 0) == self.first_warning_at_unix_ms.is_none()
    }
}

pub fn start(
    ledger: &mut Ledger,
    parent: &Actor,
    target: &Actor,
    activity: u64,
) -> Result<Watch, String> {
    parent.require_native_identity()?;
    if let Some(watch) = ledger
        .watches
        .iter()
        .find(|watch| watch.parent.same_identity(parent) && watch.target.same_identity(target))
    {
        return Ok(watch.clone());
    }
    if ledger.watches.len() >= WATCH_LIMIT {
        return Err("capacity".into());
    }
    let sequence = ledger.next_id;
    ledger.next_id = sequence.checked_add(1).ok_or("capacity")?;
    let watch = Watch {
        id: format!("watch-{sequence}"),
        parent: parent.clone(),
        target: target.clone(),
        last_activity_at_unix_ms: activity,
        first_warning_at_unix_ms: None,
        warning_count: 0,
        activity_failures: 0,
        last_status: "unknown".into(),
        last_state_change_seq: None,
        status_changed_at_unix_ms: activity,
        generation: 0,
    };
    ledger.watches.push(watch.clone());
    Ok(watch)
}

pub fn stop(ledger: &mut Ledger, parent: &Actor, id: &str) -> Result<(), String> {
    let index = ledger
        .watches
        .iter()
        .position(|watch| watch.id == id && watch.parent.same_identity(parent))
        .ok_or("watch_unavailable")?;
    ledger.watches.remove(index);
    Ok(())
}

pub fn assign(
    ledger: &mut Ledger,
    parent: &Actor,
    id: &str,
    observer: &Actor,
    expected_generation: Option<u64>,
    approval: Option<&str>,
) -> Result<Watch, String> {
    observer.require_native_identity()?;
    if approval.is_some_and(|text| !valid_approval(text)) {
        return Err("invalid_approval".into());
    }
    let approved_observer = parent.same_identity(observer) && approval.is_some();
    if approved_observer && expected_generation.is_none() {
        return Err("expected_generation_required".into());
    }
    let registered = crate::coordination::resolve_actor(ledger, id).cloned();
    let mut matches = ledger.watches.iter().enumerate().filter(|(_, watch)| {
        (watch.parent.same_identity(parent) || approved_observer)
            && (watch.id == id
                || registered
                    .as_ref()
                    .is_some_and(|target| watch.target.same_identity(target)))
    });
    let index = matches
        .next()
        .map(|(index, _)| index)
        .ok_or("watch_unavailable")?;
    if matches.next().is_some() {
        return Err("watch_ambiguous".into());
    }
    let watch = &mut ledger.watches[index];
    if expected_generation.is_some_and(|expected| expected != watch.generation) {
        return Err("stale_generation".into());
    }
    if !watch.parent.same_identity(observer) {
        watch.generation = watch.generation.checked_add(1).ok_or("capacity")?;
        watch.parent = observer.clone();
    }
    Ok(watch.clone())
}

/// Approval permits only the attested new observer to accept a handover.
/// The explicit text is bounded input and is never emitted to diagnostics.
pub fn valid_approval(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

#[derive(Clone)]
pub struct Reading {
    pub id: String,
    pub status: String,
    pub status_available: bool,
    pub state_change_seq: Option<u64>,
    pub status_changed_at_unix_ms: u64,
    pub session_modified_at_unix_ms: Option<u64>,
    pub failure: Option<String>,
    pub gone: bool,
}

#[derive(Default)]
pub struct Tick {
    pub transitions: bool,
    pub failures: Vec<(String, u32)>,
    pub rejections: Vec<(String, String)>,
}

/// A failed metadata read uses only this tick's status evidence. The durable
/// highest accepted activity identifies the episode, never a fallback read.
/// Each warning and its ordinal are admitted together; a rejected warning
/// cannot undo another target's exit or activity reset.
pub fn tick(ledger: &mut Ledger, readings: &[Reading], now: u64) -> Result<Tick, String> {
    let mut result = Tick::default();
    // One full encoding per tick. Subsequent bounded row encodings account for
    // exact JSON growth without cloning/serializing the ledger per target.
    let mut size = ledger.bytes()?.len();
    for reading in readings.iter().filter(|reading| reading.gone) {
        if let Some(index) = ledger
            .watches
            .iter()
            .position(|watch| watch.id == reading.id)
        {
            size -= encoded_len(&ledger.watches[index])? + usize::from(ledger.watches.len() > 1);
            ledger.watches.remove(index);
            result.transitions = true;
        }
    }
    let mut admitted = Vec::with_capacity(readings.len());
    for reading in readings.iter().filter(|reading| !reading.gone) {
        let Some(index) = ledger
            .watches
            .iter()
            .position(|watch| watch.id == reading.id)
        else {
            continue;
        };
        let before = ledger.watches[index].clone();
        let watch = &mut ledger.watches[index];
        if reading.status_available
            && (watch.last_status != reading.status
                || watch.last_state_change_seq != reading.state_change_seq)
        {
            watch.last_status = reading.status.clone();
            watch.last_state_change_seq = reading.state_change_seq;
            watch.status_changed_at_unix_ms = reading.status_changed_at_unix_ms.min(now);
        }
        let activity = if reading.status_available {
            reading
                .session_modified_at_unix_ms
                .unwrap_or(0)
                .max(watch.status_changed_at_unix_ms)
                .min(now)
        } else {
            watch.last_activity_at_unix_ms
        };
        watch.activity_failures = if reading.failure.is_some() {
            watch.activity_failures.saturating_add(1)
        } else {
            0
        };
        if activity > watch.last_activity_at_unix_ms {
            watch.last_activity_at_unix_ms = activity;
            watch.first_warning_at_unix_ms = None;
            watch.warning_count = 0;
        }
        let updated_size = if watch.valid() {
            resized(size, encoded_len(&before)?, encoded_len(watch)?)
        } else {
            Err("ledger_unavailable".into())
        };
        match updated_size {
            Ok(updated_size) => size = updated_size,
            Err(code) => {
                ledger.watches[index] = before;
                result.rejections.push((reading.id.clone(), code));
                continue;
            }
        }
        let watch = &ledger.watches[index];
        if watch.activity_failures >= 3 {
            result
                .failures
                .push((watch.id.clone(), watch.activity_failures));
        }
        admitted.push(reading);
    }
    for reading in admitted {
        let Some(index) = ledger
            .watches
            .iter()
            .position(|watch| watch.id == reading.id)
        else {
            continue;
        };
        let watch = &ledger.watches[index];
        let activity = if reading.status_available {
            reading
                .session_modified_at_unix_ms
                .unwrap_or(0)
                .max(watch.status_changed_at_unix_ms)
                .min(now)
        } else {
            watch.last_activity_at_unix_ms
        };
        let due = match (watch.warning_count, watch.first_warning_at_unix_ms) {
            (0, _) => now.saturating_sub(activity) >= INACTIVITY_MS,
            (1, Some(first)) => now.saturating_sub(first) >= SECOND_WARNING_MS,
            _ => false,
        };
        if !due {
            continue;
        }
        let before = watch.clone();
        let count = watch.warning_count + 1;
        let open_requests = ledger
            .letters
            .iter()
            .filter(|letter| {
                letter.waiting_answer
                    && (letter.sender.pane_id == watch.target.pane_id
                        || letter.recipient.pane_id == watch.target.pane_id)
            })
            .count();
        let basis = if !reading.status_available {
            "projection_unavailable"
        } else if reading.session_modified_at_unix_ms.is_some() {
            "session_file"
        } else {
            "status_transition_only"
        };
        let failure = reading.failure.as_deref().unwrap_or("none");
        let body = format!(
            "Watch {}: {}. Last activity unix_ms={activity}; inactive {} minutes; Herdr status={}; open requests={open_requests}; activity basis={basis}; read failure={failure}.\nInspect the target before deciding whether it is blocked.",
            watch.id,
            watch.target.name,
            now.saturating_sub(activity) / 60_000,
            if reading.status_available {
                reading.status.as_str()
            } else {
                "unavailable"
            }
        );
        let letters_before = ledger.letters.len();
        let sequence_before = ledger.next_id;
        let outcome = (|| {
            let letter = super::mailbox::send(
                ledger,
                &before.parent,
                &before.parent,
                &format!(
                    "{}:warning:{count}:{}",
                    before.id, before.last_activity_at_unix_ms
                ),
                &body,
                "watch",
                None,
                now,
            )?;
            let letter = ledger
                .letters
                .iter_mut()
                .find(|stored| stored.id == letter.id)
                .ok_or("ledger_unavailable")?;
            letter.watch_warning = Some(WarningReceipt {
                target: before.target.clone(),
                activity_at_unix_ms: before.last_activity_at_unix_ms,
                ordinal: count,
                parent_notified: false,
            });
            let watch = &mut ledger.watches[index];
            watch.first_warning_at_unix_ms.get_or_insert(now);
            watch.warning_count = count;
            let letter_growth = if ledger.letters.len() > letters_before {
                encoded_len(ledger.letters.last().ok_or("ledger_unavailable")?)?
                    + usize::from(letters_before > 0)
            } else {
                0
            };
            let next_size = resized(
                size,
                encoded_len(&before)?,
                encoded_len(&ledger.watches[index])?,
            )?;
            resized(
                next_size,
                sequence_before.to_string().len(),
                ledger.next_id.to_string().len() + letter_growth,
            )
        })();
        match outcome {
            Ok(updated_size) => {
                size = updated_size;
                result.transitions = true;
            }
            Err(code) => {
                ledger.watches[index] = before;
                ledger.letters.truncate(letters_before);
                ledger.next_id = sequence_before;
                result.rejections.push((reading.id.clone(), code));
            }
        }
    }
    Ok(result)
}

fn encoded_len(value: &impl Serialize) -> Result<usize, String> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|_| "ledger_unavailable".into())
}

fn resized(size: usize, removed: usize, added: usize) -> Result<usize, String> {
    size.checked_sub(removed)
        .and_then(|size| size.checked_add(added))
        .filter(|size| *size <= FILE_LIMIT)
        .ok_or_else(|| "capacity".into())
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
    fn reading(id: &str, activity: u64) -> Reading {
        Reading {
            id: id.into(),
            status: "idle".into(),
            status_available: true,
            state_change_seq: Some(activity),
            status_changed_at_unix_ms: activity,
            session_modified_at_unix_ms: None,
            failure: None,
            gone: false,
        }
    }

    #[test]
    fn reassigning_a_watch_preserves_activity_and_rejects_stale_or_wrong_observers() {
        let mut ledger = Ledger::default();
        let parent = actor("parent");
        let next = actor("next");
        let target = actor("target");
        let watch = start(&mut ledger, &parent, &target, 10).unwrap();
        tick(&mut ledger, &[reading(&watch.id, 10)], 10 + INACTIVITY_MS).unwrap();
        let before = ledger.watches[0].clone();
        let changed = assign(&mut ledger, &parent, &watch.id, &next, Some(0), None).unwrap();
        assert_eq!(changed.generation, 1);
        assert_eq!(
            changed.last_activity_at_unix_ms,
            before.last_activity_at_unix_ms
        );
        assert_eq!(
            changed.first_warning_at_unix_ms,
            before.first_warning_at_unix_ms
        );
        assert_eq!(changed.warning_count, before.warning_count);
        assert_eq!(
            assign(&mut ledger, &next, &watch.id, &parent, Some(0), None).unwrap_err(),
            "stale_generation"
        );
        assert_eq!(
            assign(&mut ledger, &parent, &watch.id, &parent, Some(1), None).unwrap_err(),
            "watch_unavailable"
        );
        let restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
        assert_eq!(restored.watches[0], changed);
    }

    #[test]
    fn the_current_new_observer_accepts_handover_only_with_approval_and_current_generation() {
        let mut ledger = Ledger::default();
        let parent = actor("parent");
        let next = actor("next");
        let stranger = actor("stranger");
        let watch = start(&mut ledger, &parent, &actor("target"), 10).unwrap();
        tick(&mut ledger, &[reading(&watch.id, 10)], 10 + INACTIVITY_MS).unwrap();
        let before = ledger.watches[0].clone();
        for (caller, observer, generation, approval, expected) in [
            (&next, &next, Some(0), None, "watch_unavailable"),
            (
                &next,
                &next,
                None,
                Some("approved"),
                "expected_generation_required",
            ),
            (&next, &next, Some(1), Some("approved"), "stale_generation"),
            (
                &stranger,
                &next,
                Some(0),
                Some("approved"),
                "watch_unavailable",
            ),
            (&next, &next, Some(0), Some(" "), "invalid_approval"),
        ] {
            assert_eq!(
                assign(
                    &mut ledger,
                    caller,
                    &watch.id,
                    observer,
                    generation,
                    approval
                )
                .unwrap_err(),
                expected
            );
            assert_eq!(ledger.watches[0], before);
        }
        let changed = assign(
            &mut ledger,
            &next,
            &watch.id,
            &next,
            Some(0),
            Some("approved"),
        )
        .unwrap();
        let mut expected = before;
        expected.parent = next;
        expected.generation = 1;
        assert_eq!(changed, expected);
        assert_eq!(ledger.watches[0], expected);
        assert!(
            !String::from_utf8(ledger.bytes().unwrap())
                .unwrap()
                .contains("approved")
        );
    }

    #[test]
    fn missing_target_reference_keeps_status_only_watch_without_mailbox_authority() {
        let mut ledger = Ledger::default();
        let parent = actor("parent");
        let mut target = actor("target");
        target.session = None;
        let watch = start(&mut ledger, &parent, &target, 1).unwrap();
        let mut sample = reading(&watch.id, 1);
        sample.failure = Some("session_reference_missing".into());
        let outcome = tick(&mut ledger, &[sample], 1 + INACTIVITY_MS).unwrap();
        assert!(outcome.rejections.is_empty());
        assert_eq!(ledger.watches[0].warning_count, 1);
        assert_eq!(ledger.watches[0].target.session, None);
        assert_eq!(ledger.letters[0].recipient, parent);
        assert!(ledger.letters[0].body.contains("status_transition_only"));
        assert_eq!(
            super::super::mailbox::pull(&ledger, &target).unwrap_err(),
            "native_identity_required"
        );
        let before = ledger.clone();
        assert_eq!(
            start(&mut ledger, &target, &parent, 1).unwrap_err(),
            "native_identity_required"
        );
        assert_eq!(ledger, before);
    }

    #[test]
    fn two_warnings_keep_the_first_deadline_across_restart_and_reset_on_activity() {
        let mut ledger = Ledger::default();
        let watch = start(&mut ledger, &actor("parent"), &actor("target"), 10).unwrap();
        tick(
            &mut ledger,
            &[reading(&watch.id, 10)],
            10 + INACTIVITY_MS - 1,
        )
        .unwrap();
        assert!(ledger.letters.is_empty());
        tick(&mut ledger, &[reading(&watch.id, 10)], 10 + INACTIVITY_MS).unwrap();
        assert_eq!(ledger.letters.len(), 1);
        let mut ledger: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
        tick(
            &mut ledger,
            &[reading(&watch.id, 10)],
            10 + INACTIVITY_MS + SECOND_WARNING_MS - 1,
        )
        .unwrap();
        assert_eq!(ledger.letters.len(), 1);
        tick(
            &mut ledger,
            &[reading(&watch.id, 10)],
            10 + INACTIVITY_MS + SECOND_WARNING_MS,
        )
        .unwrap();
        tick(
            &mut ledger,
            &[reading(&watch.id, 10)],
            10 + INACTIVITY_MS + SECOND_WARNING_MS * 2,
        )
        .unwrap();
        assert_eq!(ledger.letters.len(), 2);
        let active = 10 + INACTIVITY_MS + SECOND_WARNING_MS * 2 + 1;
        tick(&mut ledger, &[reading(&watch.id, active)], active).unwrap();
        assert_eq!(ledger.watches[0].warning_count, 0);
        assert_eq!(ledger.watches[0].first_warning_at_unix_ms, None);
        tick(
            &mut ledger,
            &[reading(&watch.id, active)],
            active + INACTIVITY_MS,
        )
        .unwrap();
        assert_eq!(ledger.letters.len(), 3);
    }

    #[test]
    fn bootstrap_of_unchanged_status_keeps_the_durable_activity_clock() {
        let mut ledger = Ledger::default();
        let watch = start(&mut ledger, &actor("parent"), &actor("target"), 10).unwrap();
        tick(&mut ledger, &[reading(&watch.id, 10)], 10 + INACTIVITY_MS).unwrap();
        let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
        let mut bootstrap = reading(&watch.id, 10);
        bootstrap.status_changed_at_unix_ms = 10 + INACTIVITY_MS + SECOND_WARNING_MS;
        tick(
            &mut restored,
            &[bootstrap],
            10 + INACTIVITY_MS + SECOND_WARNING_MS,
        )
        .unwrap();
        assert_eq!(restored.letters.len(), 2);
        assert_eq!(restored.watches[0].warning_count, 2);
        assert_eq!(
            restored.watches[0].first_warning_at_unix_ms,
            Some(10 + INACTIVITY_MS)
        );
    }

    #[test]
    fn reply_does_not_stop_watch_and_failed_reads_do_not_suppress_it() {
        let mut ledger = Ledger::default();
        let parent = actor("parent");
        let target = actor("target");
        let watch = start(&mut ledger, &parent, &target, 1).unwrap();
        let request = super::super::mailbox::send(
            &mut ledger,
            &parent,
            &target,
            "request",
            "work",
            "request",
            None,
            1,
        )
        .unwrap();
        super::super::mailbox::apply(
            &mut ledger,
            &target,
            None,
            &super::super::Command::Reply {
                id: request.id,
                intent: "reply".into(),
                body: "done".into(),
            },
            2,
        )
        .unwrap();
        assert_eq!(ledger.watches.len(), 1);
        let mut reading = reading(&watch.id, 1);
        reading.failure = Some("session_activity_unavailable".into());
        tick(&mut ledger, &[reading], 1 + INACTIVITY_MS).unwrap();
        assert!(
            ledger
                .letters
                .last()
                .unwrap()
                .body
                .contains("status_transition_only")
        );
        assert!(
            ledger
                .letters
                .last()
                .unwrap()
                .body
                .contains("session_activity_unavailable")
        );
        let mut reading = Reading {
            id: watch.id.clone(),
            status: "unknown".into(),
            status_available: true,
            state_change_seq: None,
            status_changed_at_unix_ms: 1,
            session_modified_at_unix_ms: None,
            failure: None,
            gone: false,
        };
        reading.gone = true;
        tick(&mut ledger, &[reading], 2 + INACTIVITY_MS).unwrap();
        assert!(ledger.watches.is_empty());
    }

    fn closed_letter(
        ledger: &mut Ledger,
        body: String,
        open: bool,
        now: u64,
    ) -> super::super::ledger::Letter {
        let sequence = ledger.next_id;
        ledger.next_id += 1;
        super::super::ledger::Letter {
            id: format!("letter-{sequence}"),
            intent: format!("fill-{sequence}"),
            sender: actor("filler"),
            recipient: actor("filler"),
            kind: "request".into(),
            body,
            state: if open {
                super::super::ledger::State::Delivered
            } else {
                super::super::ledger::State::Acknowledged
            },
            hook_confirmed: Some(true),
            waiting_answer: open,
            reply_to: None,
            created_at_unix_ms: now,
            finished_at_unix_ms: (!open).then_some(now),
            bell_errors: 0,
            bell_sent: false,
            bell_attempts: Some(0),
            human_notified: false,
            watch_warning: None,
        }
    }

    fn fill_file_cap(ledger: &mut Ledger, now: u64) {
        let limit = 16 * 1024 * 1024 - 1;
        let mut size = ledger.bytes().unwrap().len();
        while size < limit {
            let sequence = ledger.next_id;
            let mut letter = closed_letter(ledger, "x".into(), false, now);
            let overhead = encoded_len(&letter).unwrap()
                + usize::from(!ledger.letters.is_empty())
                + ledger.next_id.to_string().len()
                - sequence.to_string().len();
            let mut room = limit - size;
            if room < overhead {
                let removed = overhead - room;
                let last = ledger.letters.last_mut().unwrap();
                last.body.truncate(last.body.len() - removed);
                size -= removed;
                room += removed;
            }
            letter.body = "x".repeat(1 + (room - overhead).min(16 * 1024 - 1));
            size += overhead + letter.body.len() - 1;
            ledger.letters.push(letter);
        }
        assert_eq!(ledger.bytes().unwrap().len(), limit);
    }

    #[test]
    fn warning_capacity_preserves_other_target_exit_and_activity_reset() {
        for cap in ["open", "retained", "file"] {
            for reverse in [false, true] {
                let mut ledger = Ledger::default();
                let parent = actor("parent");
                let gone = start(&mut ledger, &parent, &actor("gone"), 1).unwrap();
                let reset = start(&mut ledger, &parent, &actor("resumed"), 1).unwrap();
                let due = start(&mut ledger, &parent, &actor("inactive"), 1).unwrap();
                for watch in &mut ledger.watches {
                    watch.last_status = "idle".into();
                    watch.last_state_change_seq = Some(1);
                }
                ledger.watches[1].warning_count = 1;
                ledger.watches[1].first_warning_at_unix_ms = Some(10);
                let now = 1 + INACTIVITY_MS;
                match cap {
                    "open" | "retained" => {
                        let count = if cap == "open" { 1024 } else { 5000 };
                        for _ in 0..count {
                            let letter =
                                closed_letter(&mut ledger, "retained".into(), cap == "open", now);
                            ledger.letters.push(letter);
                        }
                    }
                    "file" => fill_file_cap(&mut ledger, now),
                    _ => unreachable!(),
                }
                let original_letters = ledger.letters.clone();
                let original_sequence = ledger.next_id;
                let mut exit = reading(&gone.id, 1);
                exit.gone = true;
                let mut resumed = reading(&reset.id, 1);
                resumed.session_modified_at_unix_ms = Some(now);
                let mut readings = vec![exit, resumed, reading(&due.id, 1)];
                if reverse {
                    readings.reverse();
                }
                let outcome = tick(&mut ledger, &readings, now).unwrap();
                assert!(outcome.transitions);
                assert!(
                    outcome
                        .rejections
                        .contains(&(due.id.clone(), "capacity".into())),
                    "{cap}"
                );
                assert!(!ledger.watches.iter().any(|watch| watch.id == gone.id));
                let reset = ledger
                    .watches
                    .iter()
                    .find(|watch| watch.id == reset.id)
                    .unwrap();
                assert_eq!(reset.warning_count, 0);
                assert_eq!(reset.first_warning_at_unix_ms, None);
                assert_eq!(reset.last_activity_at_unix_ms, now);
                let due = ledger
                    .watches
                    .iter()
                    .find(|watch| watch.id == due.id)
                    .unwrap();
                assert_eq!(due.warning_count, 0);
                assert_eq!(due.first_warning_at_unix_ms, None);
                assert_eq!(ledger.letters, original_letters);
                assert_eq!(ledger.next_id, original_sequence);
                ledger.bytes().unwrap();
            }
        }
    }

    #[test]
    fn file_only_activity_reset_and_failed_read_start_a_distinct_durable_warning_episode() {
        for cancel in [false, true] {
            let mut ledger = Ledger::default();
            let parent = actor("parent");
            let watch = start(&mut ledger, &parent, &actor("target"), 1).unwrap();
            let first = 1 + INACTIVITY_MS;
            tick(&mut ledger, &[reading(&watch.id, 1)], first).unwrap();
            let old_id = ledger.letters[0].id.clone();
            let command = if cancel {
                super::super::Command::Cancel { id: old_id.clone() }
            } else {
                super::super::Command::Confirm {
                    ids: vec![old_id.clone()],
                }
            };
            super::super::mailbox::apply(&mut ledger, &parent, None, &command, first).unwrap();
            let accepted_activity = first + 1;
            let mut resumed = reading(&watch.id, 1);
            resumed.session_modified_at_unix_ms = Some(accepted_activity);
            tick(&mut ledger, &[resumed], accepted_activity).unwrap();
            assert_eq!(ledger.watches[0].warning_count, 0);
            let mut failed = reading(&watch.id, 1);
            failed.failure = Some("session_activity_unavailable".into());
            let new_first = accepted_activity + INACTIVITY_MS;
            tick(&mut ledger, &[failed.clone()], new_first).unwrap();
            assert_eq!(ledger.letters.len(), 2);
            assert_ne!(ledger.letters[1].id, old_id);
            assert!(ledger.letters[1].body.contains("status_transition_only"));
            let new_id = ledger.letters[1].id.clone();
            let mut restored: Ledger = serde_json::from_slice(&ledger.bytes().unwrap()).unwrap();
            tick(
                &mut restored,
                &[failed.clone()],
                new_first + SECOND_WARNING_MS - 1,
            )
            .unwrap();
            assert_eq!(restored.letters.len(), 2);
            tick(
                &mut restored,
                &[failed.clone()],
                new_first + SECOND_WARNING_MS,
            )
            .unwrap();
            assert_eq!(restored.letters.len(), 3);
            assert_ne!(restored.letters[2].id, new_id);
            tick(&mut restored, &[failed], new_first + SECOND_WARNING_MS * 2).unwrap();
            assert_eq!(restored.letters.len(), 3);
            assert_eq!(
                restored.watches[0].first_warning_at_unix_ms,
                Some(new_first)
            );
            assert_eq!(restored.watches[0].warning_count, 2);
        }
    }
}
