use serde::{Deserialize, Serialize};

use super::ledger::Ledger;
use super::{Actor, INACTIVITY_MS, SECOND_WARNING_MS, WATCH_LIMIT};

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
            && self.parent.device_id == "local"
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
    if parent.device_id != "local" {
        return Err("local_parent_required".into());
    }
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

#[derive(Clone)]
pub struct Reading {
    pub id: String,
    pub status: String,
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
}

/// A failed metadata read uses only this tick's status evidence. The previous
/// session timestamp detects resumed activity; it is never a fallback read.
/// The warning and its count commit together in the caller's ledger transaction.
pub fn tick(ledger: &mut Ledger, readings: &[Reading], now: u64) -> Result<Tick, String> {
    let mut result = Tick::default();
    for reading in readings {
        let Some(index) = ledger
            .watches
            .iter()
            .position(|watch| watch.id == reading.id)
        else {
            continue;
        };
        if reading.gone {
            ledger.watches.remove(index);
            result.transitions = true;
            continue;
        }
        let watch = &mut ledger.watches[index];
        if watch.last_status != reading.status
            || watch.last_state_change_seq != reading.state_change_seq
        {
            watch.last_status = reading.status.clone();
            watch.last_state_change_seq = reading.state_change_seq;
            watch.status_changed_at_unix_ms = reading.status_changed_at_unix_ms.min(now);
        }
        let activity = reading
            .session_modified_at_unix_ms
            .unwrap_or(0)
            .max(watch.status_changed_at_unix_ms)
            .min(now);
        if reading.failure.is_some() {
            watch.activity_failures = watch.activity_failures.saturating_add(1);
            if watch.activity_failures >= 3 {
                result
                    .failures
                    .push((watch.id.clone(), watch.activity_failures));
            }
        } else {
            watch.activity_failures = 0;
        }
        if activity > watch.last_activity_at_unix_ms {
            watch.last_activity_at_unix_ms = activity;
            watch.first_warning_at_unix_ms = None;
            watch.warning_count = 0;
        }
        let due = match (watch.warning_count, watch.first_warning_at_unix_ms) {
            (0, _) => now.saturating_sub(activity) >= INACTIVITY_MS,
            (1, Some(first)) => now.saturating_sub(first) >= SECOND_WARNING_MS,
            _ => false,
        };
        if !due {
            continue;
        }
        let parent = watch.parent.clone();
        let id = watch.id.clone();
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
        let basis = if reading.session_modified_at_unix_ms.is_some() {
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
            reading.status
        );
        super::mailbox::send(
            ledger,
            &parent,
            &parent,
            &format!("{id}:warning:{count}:{activity}"),
            &body,
            "watch",
            None,
            now,
        )?;
        let watch = &mut ledger.watches[index];
        watch.first_warning_at_unix_ms.get_or_insert(now);
        watch.warning_count = count;
        result.transitions = true;
    }
    Ok(result)
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
            session: None,
        }
    }
    fn reading(id: &str, activity: u64) -> Reading {
        Reading {
            id: id.into(),
            status: "idle".into(),
            state_change_seq: Some(activity),
            status_changed_at_unix_ms: activity,
            session_modified_at_unix_ms: None,
            failure: None,
            gone: false,
        }
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
}
