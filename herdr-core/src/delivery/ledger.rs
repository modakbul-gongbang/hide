use std::collections::HashSet;
use std::io::Read;
use std::path::{Component, Path};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::watch::Watch;
use super::{
    ANSWER_WAIT_MS, Actor, BODY_LIMIT, FILE_LIMIT, LETTER_LIMIT, OPEN_LIMIT, RETENTION_MS,
    WATCH_LIMIT,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Pending,
    Delivered,
    Acknowledged,
    Cancelled,
    Expired,
    Undelivered,
}

/// Why a letter stopped waiting for an answer nobody gave.
// The letter keeps its state: a new `State` value would make an older build
// refuse the whole ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnswerWaitEnd {
    /// The sender's or the recipient's registration ended.
    PartyEnded,
    /// The letter was sent 24 hours ago and no answer came.
    Deadline,
}

impl AnswerWaitEnd {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PartyEnded => "party_ended",
            Self::Deadline => "deadline",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Letter {
    pub id: String,
    pub intent: String,
    pub sender: Actor,
    pub recipient: Actor,
    pub kind: String,
    pub body: String,
    pub state: State,
    #[serde(default)]
    pub hook_confirmed: Option<bool>,
    pub waiting_answer: bool,
    pub reply_to: Option<String>,
    pub created_at_unix_ms: u64,
    pub finished_at_unix_ms: Option<u64>,
    pub bell_errors: u8,
    pub bell_sent: bool,
    #[serde(default)]
    pub bell_attempts: Option<u8>,
    #[serde(default)]
    pub human_notified: bool,
    #[serde(default)]
    pub watch_warning: Option<super::watch::WarningReceipt>,
    /// Set when the wait for an answer ended without one; null otherwise.
    #[serde(default)]
    pub answer_wait_ended: Option<AnswerWaitEnd>,
}

impl Letter {
    pub fn open(&self) -> bool {
        self.awaiting_intake() || self.waiting_answer
    }

    pub(crate) fn intake_confirmed(&self) -> bool {
        // Only legacy Delivered proves a hook confirmation. A legacy Ack
        // may have preceded intake, so its missing receipt remains unknown.
        self.hook_confirmed
            .unwrap_or(self.state == State::Delivered)
    }

    pub(crate) fn awaiting_intake(&self) -> bool {
        self.state == State::Pending
            || (self.state == State::Acknowledged && self.hook_confirmed == Some(false))
    }

    /// Whether the delivery deadline passed while the letter still waits for
    /// intake; `Ledger::expire` ends that wait.
    pub(crate) fn intake_overdue(&self, now: u64) -> bool {
        self.awaiting_intake()
            && now.saturating_sub(self.created_at_unix_ms) >= super::DELIVERY_EXPIRY_MS
    }

    /// Whether the answer is still awaited past its deadline.
    pub(crate) fn answer_overdue(&self, now: u64) -> bool {
        self.waiting_answer && now.saturating_sub(self.created_at_unix_ms) >= ANSWER_WAIT_MS
    }

    /// Ends the wait for an answer. A letter still awaiting intake stays open
    /// for that, and `finished_at_unix_ms` is set once nothing else is awaited.
    fn end_answer_wait(&mut self, why: AnswerWaitEnd, now: u64) {
        self.waiting_answer = false;
        self.answer_wait_ended = Some(why);
        if !self.open() {
            self.finished_at_unix_ms.get_or_insert(now);
        }
    }

    pub(crate) fn attempts(&self) -> u8 {
        // Legacy successes have an unknown total. Do not infer spare attempts
        // from the old boolean and repeat an already unbounded external effect.
        self.bell_attempts
            .unwrap_or(if self.bell_sent { 3 } else { self.bell_errors })
    }

    pub(crate) fn reserve_bell(&mut self, now: u64) -> Result<u8, String> {
        self.recipient.require_native_identity()?;
        if self.state != State::Pending
            || now.saturating_sub(self.created_at_unix_ms) >= super::DELIVERY_EXPIRY_MS
        {
            return Err("letter_not_pending".into());
        }
        if self.attempts() >= 3 {
            return Err("doorbell_attempt_limit".into());
        }
        let attempt = self.attempts() + 1;
        self.bell_attempts = Some(attempt);
        Ok(attempt)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    pub version: u32,
    pub next_id: u64,
    pub letters: Vec<Letter>,
    pub watches: Vec<Watch>,
    #[serde(default)]
    pub agents: Vec<crate::coordination::AgentRecord>,
    #[serde(default)]
    pub spawns: Vec<crate::coordination::SpawnRecord>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            version: 1,
            next_id: 1,
            letters: Vec::new(),
            watches: Vec::new(),
            agents: Vec::new(),
            spawns: Vec::new(),
        }
    }
}

impl Ledger {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.next_id == 0 {
            return Err("ledger_unavailable".into());
        }
        if self.letters.len() > LETTER_LIMIT
            || self.letters.iter().filter(|letter| letter.open()).count() > OPEN_LIMIT
            || self.watches.len() > WATCH_LIMIT
        {
            return Err("capacity".into());
        }
        let mut ids = HashSet::new();
        for letter in &self.letters {
            if !ids.insert(&letter.id)
                || !super::valid_key(&letter.id)
                || !letter.id.starts_with("letter-")
                || !super::valid_key(&letter.intent)
                || !letter.sender.valid()
                || !letter.recipient.valid()
                || !matches!(
                    letter.kind.as_str(),
                    "request" | "block" | "report" | "reply" | "watch"
                )
                || letter.body.len() > BODY_LIMIT
                || letter.body.trim().is_empty()
                || letter.bell_errors > 3
                || letter.bell_attempts.is_some_and(|attempts| attempts > 3)
                || (letter.hook_confirmed == Some(true)
                    && matches!(
                        letter.state,
                        State::Pending | State::Undelivered | State::Expired
                    ))
                || (letter.hook_confirmed == Some(false) && letter.state == State::Delivered)
                || letter
                    .watch_warning
                    .as_ref()
                    .is_some_and(|warning| letter.kind != "watch" || !warning.valid())
                || (!letter.open() && letter.finished_at_unix_ms.is_none())
                || (letter.waiting_answer && letter.answer_wait_ended.is_some())
                || (matches!(
                    letter.state,
                    State::Cancelled | State::Undelivered | State::Expired
                ) && letter.waiting_answer)
            {
                return Err("ledger_unavailable".into());
            }
        }
        let mut watches = HashSet::new();
        for watch in &self.watches {
            if !watches.insert(&watch.id) || !watch.valid() || !watch.id.starts_with("watch-") {
                return Err("ledger_unavailable".into());
            }
        }
        crate::coordination::validate_records(self)?;
        let mut sequences = HashSet::new();
        for id in ids
            .into_iter()
            .chain(watches)
            .chain(self.agents.iter().map(|record| &record.id))
            .chain(self.spawns.iter().map(|record| &record.id))
        {
            let sequence = id
                .split_once('-')
                .and_then(|(_, value)| value.parse::<u64>().ok())
                .ok_or("ledger_unavailable")?;
            if sequence == 0 || sequence >= self.next_id || !sequences.insert(sequence) {
                return Err("ledger_unavailable".into());
            }
        }
        Ok(())
    }

    pub fn cleanup(&mut self, now: u64) -> bool {
        let before = self.letters.len();
        self.letters.retain(|letter| {
            letter.open()
                || letter
                    .finished_at_unix_ms
                    .is_none_or(|finished| now.saturating_sub(finished) < RETENTION_MS)
        });
        self.letters.len() != before
    }

    pub fn expire(&mut self, now: u64) -> bool {
        let mut changed = self.cleanup(now);
        for letter in &mut self.letters {
            if !letter.intake_overdue(now) {
                continue;
            }
            if letter.state == State::Pending {
                letter.state = State::Undelivered;
                letter.waiting_answer = false;
                letter.finished_at_unix_ms = Some(now);
            } else {
                // Acknowledged without a receipt by a build before the
                // acknowledgement was the receipt. Its intake is no longer
                // awaited: the hook stops handing it over and it stops
                // counting as open, while an awaited reply keeps it open.
                letter.hook_confirmed = None;
                if !letter.open() {
                    letter.finished_at_unix_ms.get_or_insert(now);
                }
            }
            changed = true;
        }
        changed
    }

    /// Ends the answer wait of every letter `actor` sent or received and the
    /// recipient took in, because its registration ended. A letter still
    /// awaiting intake is left to the delivery deadline. Returns whether any
    /// wait ended.
    pub(crate) fn end_answer_waits_of(&mut self, actor: &Actor, now: u64) -> bool {
        let mut ended = false;
        for letter in &mut self.letters {
            if letter.waiting_answer
                && !letter.awaiting_intake()
                && (letter.sender.same_identity(actor) || letter.recipient.same_identity(actor))
            {
                letter.end_answer_wait(AnswerWaitEnd::PartyEnded, now);
                ended = true;
            }
        }
        ended
    }

    /// Ends the answer wait of every letter sent `ANSWER_WAIT_MS` ago.
    pub fn end_overdue_answer_waits(&mut self, now: u64) -> bool {
        let mut ended = false;
        for letter in &mut self.letters {
            if letter.answer_overdue(now) {
                letter.end_answer_wait(AnswerWaitEnd::Deadline, now);
                ended = true;
            }
        }
        ended
    }

    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| "ledger_unavailable".to_owned())?;
        if bytes.len() > FILE_LIMIT {
            return Err("capacity".into());
        }
        Ok(bytes)
    }
}

pub fn load(path: &Path) -> Result<Ledger, String> {
    let file = match hide_platform::fs::private::open_own_file(path, false) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Ledger::default()),
        Err(_) => return Err("ledger_unavailable".into()),
    };
    if !hide_platform::fs::private::is_private(path).unwrap_or(false)
        || file.metadata().map_err(|_| "ledger_unavailable")?.len() > FILE_LIMIT as u64
    {
        return Err("ledger_unavailable".into());
    }
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "ledger_unavailable")?;
    if bytes.len() > FILE_LIMIT {
        return Err("ledger_unavailable".into());
    }
    let ledger: Ledger = serde_json::from_slice(&bytes).map_err(|_| "ledger_unavailable")?;
    ledger.validate().map_err(|_| "ledger_unavailable")?;
    Ok(ledger)
}

/// Keep the persistence phase intact until the store has applied its
/// admission policy. An uncertain replacement can never mean old bytes won.
#[derive(Debug)]
pub enum SaveError {
    Validation(String),
    Persistence(hide_platform::fs::atomic::DurableWriteError),
    Bootstrap(hide_platform::fs::private::DirectoryDurabilityError),
}

impl SaveError {
    pub fn code(&self) -> &str {
        match self {
            Self::Validation(code) => code,
            Self::Persistence(_) | Self::Bootstrap(_) => "ledger_unavailable",
        }
    }

    pub fn uncertain(&self) -> bool {
        matches!(
            self,
            Self::Persistence(
                hide_platform::fs::atomic::DurableWriteError::ReplacementUncertain { .. }
                    | hide_platform::fs::atomic::DurableWriteError::ReplacedNotDurable { .. }
            ) | Self::Bootstrap(
                hide_platform::fs::private::DirectoryDurabilityError::Uncertain { .. }
            )
        )
    }

    pub fn diagnostic(&self) -> serde_json::Value {
        use hide_platform::fs::atomic::DurableWriteError;
        let (phase, source, cleanup) = match self {
            Self::Validation(code) => return serde_json::json!({"phase":"validation","code":code}),
            Self::Bootstrap(error) => {
                use hide_platform::fs::private::DirectoryDurabilityError;
                let (phase, source) = match error {
                    DirectoryDurabilityError::BeforeCreate { source } => {
                        ("bootstrap_refused", source)
                    }
                    DirectoryDurabilityError::Uncertain { source, .. } => {
                        ("bootstrap_uncertain", source)
                    }
                };
                return serde_json::json!({"phase":phase,"io_kind":format!("{:?}",source.kind())});
            }
            Self::Persistence(DurableWriteError::BeforeReplace { source, cleanup }) => {
                ("before_replace", source, cleanup.is_some())
            }
            Self::Persistence(DurableWriteError::ReplacementUncertain { source, cleanup }) => {
                ("replacement_uncertain", source, cleanup.is_some())
            }
            Self::Persistence(DurableWriteError::ReplacedNotDurable { source, .. }) => {
                ("replaced_not_durable", source, false)
            }
        };
        // IO text and temporary paths can contain private directory names.
        serde_json::json!({"phase":phase,"io_kind":format!("{:?}",source.kind()),"cleanup_failed":cleanup})
    }
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for SaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(_) => None,
            Self::Persistence(source) => Some(source),
            Self::Bootstrap(source) => Some(source),
        }
    }
}

pub fn save(path: &Path, ledger: &Ledger) -> Result<(), SaveError> {
    let bytes = ledger.bytes().map_err(SaveError::Validation)?;
    // The parent and its durable ancestry are established by initialization,
    // never opportunistically created by a transaction after admission.
    hide_platform::fs::atomic::write_file_durable(path, &bytes, hide_platform::fs::Access::Private)
        .map_err(SaveError::Persistence)?;
    Ok(())
}

/// Startup runs before Runtime is placed behind its mutex. Re-read the
/// actual installed version and establish a new barrier before any effect.
/// Corrupt or inaccessible bytes are preserved and never repaired by guess.
pub fn recover(path: &Path) -> Result<Ledger, SaveError> {
    let refused = || SaveError::Validation("ledger_unavailable".into());
    let parent = path.parent().ok_or_else(refused)?;
    if !path.is_absolute()
        || path.components().count() > 66
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(refused());
    }
    match std::fs::symlink_metadata(parent) {
        Ok(metadata) => {
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || !hide_platform::fs::private::is_private(parent).map_err(|_| refused())?
            {
                return Err(refused());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(refused()),
    }
    let ledger = load(path).map_err(SaveError::Validation)?;
    let mut ancestor = parent;
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if missing.len() == 64 {
                    return Err(refused());
                }
                missing.push(ancestor.file_name().ok_or_else(refused)?);
                ancestor = ancestor.parent().ok_or_else(refused)?;
            }
            Err(_) => return Err(refused()),
        }
    }
    let anchor = hide_platform::fs::private::establish_dir_durability(ancestor)
        .map_err(SaveError::Bootstrap)?;
    let mut canonical_parent = anchor.clone();
    for part in missing.into_iter().rev() {
        canonical_parent.push(part);
    }
    hide_platform::fs::private::create_dir_all_durable(&canonical_parent, &anchor)
        .map_err(SaveError::Bootstrap)?;
    if !hide_platform::fs::private::is_private(&canonical_parent).map_err(|_| refused())? {
        return Err(refused());
    }
    save(path, &ledger)?;
    Ok(ledger)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letter() -> Ledger {
        let mut ledger = Ledger::default();
        let sender = Actor {
            pane_id: "sender".into(),
            name: "sender".into(),
            kind: "codex".into(),
            device_id: "local".into(),
            session: Some("sender-native".into()),
        };
        let recipient = Actor {
            pane_id: "recipient".into(),
            name: "recipient".into(),
            kind: "codex".into(),
            device_id: "local".into(),
            session: Some("recipient-native".into()),
        };
        super::super::mailbox::send(
            &mut ledger,
            &sender,
            &recipient,
            "once",
            "private",
            "request",
            None,
            1,
        )
        .unwrap();
        ledger
    }

    #[test]
    fn total_doorbell_reservations_include_success_and_survive_restart_before_confirmation() {
        for sent in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("ledger.json");
            let mut ledger = letter();
            for attempt in 1..=3 {
                assert_eq!(ledger.letters[0].reserve_bell(2).unwrap(), attempt);
                save(&path, &ledger).unwrap();
                // A lost response or a crash here still consumes the reservation.
                ledger = load(&path).unwrap();
                assert_eq!(ledger.letters[0].attempts(), attempt);
                assert_eq!(ledger.letters[0].state, State::Pending);
                if sent {
                    ledger.letters[0].bell_sent = true;
                } else {
                    ledger.letters[0].bell_errors += 1;
                }
                save(&path, &ledger).unwrap();
                ledger = load(&path).unwrap();
            }
            let before = ledger.bytes().unwrap();
            assert_eq!(
                ledger.letters[0].reserve_bell(2).unwrap_err(),
                "doorbell_attempt_limit"
            );
            assert_eq!(ledger.bytes().unwrap(), before);
            let recipient = ledger.letters[0].recipient.clone();
            let id = ledger.letters[0].id.clone();
            super::super::mailbox::apply(
                &mut ledger,
                &recipient,
                None,
                &super::super::Command::Confirm { ids: vec![id] },
                2,
            )
            .unwrap();
            save(&path, &ledger).unwrap();
            ledger = load(&path).unwrap();
            assert_eq!(
                ledger.letters[0].reserve_bell(2).unwrap_err(),
                "letter_not_pending"
            );
            assert!(
                super::super::mailbox::pull(&ledger, &recipient)
                    .unwrap()
                    .ids
                    .is_empty()
            );
        }
    }

    #[test]
    fn legacy_success_does_not_infer_spare_attempts_and_invalid_budget_is_rejected() {
        let ledger = letter();
        for sent in [true, false] {
            let mut record = serde_json::to_value(&ledger).unwrap();
            let row = record["letters"][0].as_object_mut().unwrap();
            row.remove("bell_attempts");
            row.insert("bell_sent".into(), serde_json::json!(sent));
            row.insert("bell_errors".into(), serde_json::json!(2));
            let mut restored: Ledger = serde_json::from_value(record).unwrap();
            if sent {
                assert_eq!(
                    restored.letters[0].reserve_bell(2).unwrap_err(),
                    "doorbell_attempt_limit"
                );
            } else {
                assert_eq!(restored.letters[0].reserve_bell(2).unwrap(), 3);
                assert_eq!(
                    restored.letters[0].reserve_bell(2).unwrap_err(),
                    "doorbell_attempt_limit"
                );
            }
        }
        let mut invalid = ledger;
        invalid.letters[0].bell_attempts = Some(4);
        assert_eq!(invalid.validate().unwrap_err(), "ledger_unavailable");
    }

    #[test]
    fn acknowledgement_receipt_survives_durable_reload_and_replay_without_closing_new_watch() {
        use super::super::{mailbox, watch};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("ledger.json");
        let mut ledger = letter();
        ledger.letters[0].kind = "report".into();
        ledger.letters[0].waiting_answer = false;
        let child = ledger.letters[0].sender.clone();
        let parent = ledger.letters[0].recipient.clone();
        let id = ledger.letters[0].id.clone();
        watch::start(&mut ledger, &parent, &child, 2).unwrap();
        let intake = mailbox::pull(&ledger, &parent).unwrap();
        mailbox::apply(
            &mut ledger,
            &parent,
            None,
            &mailbox::Command::Ack { id: id.clone() },
            3,
        )
        .unwrap();
        save(&path, &ledger).unwrap();
        let mut restored = load(&path).unwrap();
        assert!(restored.watches.is_empty());
        assert!(mailbox::pull(&restored, &parent).unwrap().ids.is_empty());
        let shown = mailbox::apply(
            &mut restored,
            &parent,
            None,
            &mailbox::Command::Show { id },
            4,
        )
        .unwrap();
        assert_eq!(shown["state"], "acknowledged");
        assert_eq!(shown["hook_confirmed"], true);
        watch::start(&mut restored, &parent, &child, 5).unwrap();
        save(&path, &restored).unwrap();
        let mut restored = load(&path).unwrap();
        let before = restored.clone();
        let confirm = mailbox::Command::Confirm { ids: intake.ids };
        mailbox::apply(&mut restored, &parent, None, &confirm, 6).unwrap();
        save(&path, &restored).unwrap();
        assert_eq!(load(&path).unwrap(), before);
    }

    #[test]
    fn legacy_delivered_proves_intake_but_legacy_ack_stays_closed_without_delivery_proof() {
        use super::super::{mailbox, watch};
        for state in [State::Delivered, State::Acknowledged] {
            let mut fixture = serde_json::to_value(letter()).unwrap();
            let row = fixture["letters"][0].as_object_mut().unwrap();
            row.remove("hook_confirmed");
            row.insert("state".into(), serde_json::to_value(state).unwrap());
            row.insert("kind".into(), serde_json::json!("report"));
            row.insert("waiting_answer".into(), serde_json::json!(false));
            row.insert("finished_at_unix_ms".into(), serde_json::json!(2));
            let mut restored: Ledger = serde_json::from_value(fixture).unwrap();
            restored.validate().unwrap();
            let explicit_null: Ledger =
                serde_json::from_value(serde_json::to_value(&restored).unwrap()).unwrap();
            assert_eq!(explicit_null, restored);
            let child = restored.letters[0].sender.clone();
            let parent = restored.letters[0].recipient.clone();
            let id = restored.letters[0].id.clone();
            let watched = watch::start(&mut restored, &parent, &child, 3).unwrap();
            let shown = mailbox::apply(
                &mut restored,
                &parent,
                None,
                &mailbox::Command::Show { id: id.clone() },
                4,
            )
            .unwrap();
            assert!(shown["hook_confirmed"].is_null());
            assert!(!restored.letters[0].open());
            assert!(mailbox::pull(&restored, &parent).unwrap().ids.is_empty());
            let mut retained = restored.clone();
            assert!(retained.cleanup(RETENTION_MS + 3));
            assert!(retained.letters.is_empty());
            assert_eq!(retained.watches[0].id, watched.id);
            if state == State::Delivered {
                assert!(restored.letters[0].intake_confirmed());
                mailbox::apply(
                    &mut restored,
                    &parent,
                    None,
                    &mailbox::Command::Confirm { ids: vec![id] },
                    5,
                )
                .unwrap();
                assert_eq!(restored.watches[0].id, watched.id);
                assert!(restored.letters[0].hook_confirmed.is_none());
            } else {
                assert!(!restored.letters[0].intake_confirmed());
                assert_eq!(restored.watches[0].id, watched.id);
                mailbox::apply(
                    &mut restored,
                    &parent,
                    None,
                    &mailbox::Command::Confirm { ids: vec![id] },
                    5,
                )
                .unwrap();
                assert!(restored.watches.is_empty());
                assert_eq!(restored.letters[0].hook_confirmed, Some(true));
            }
        }
        for (state, receipt) in [
            (State::Pending, true),
            (State::Undelivered, true),
            (State::Expired, true),
            (State::Delivered, false),
        ] {
            let mut invalid = letter();
            invalid.letters[0].state = state;
            invalid.letters[0].hook_confirmed = Some(receipt);
            invalid.letters[0].waiting_answer = false;
            invalid.letters[0].finished_at_unix_ms = Some(2);
            assert_eq!(invalid.validate().unwrap_err(), "ledger_unavailable");
        }
        for receipt in [
            serde_json::json!("true"),
            serde_json::json!(1),
            serde_json::json!({}),
        ] {
            let mut invalid = serde_json::to_value(letter()).unwrap();
            invalid["letters"][0]["hook_confirmed"] = receipt;
            assert!(serde_json::from_value::<Ledger>(invalid).is_err());
        }
    }

    #[test]
    fn validated_startup_reestablishes_the_installed_version_without_repairing_corruption() {
        let root = tempfile::tempdir().unwrap();
        // A tempfile root uses the normal directory default; the product
        // stores its ledger inside a private state directory.
        let state = root.path().join("state");
        hide_platform::fs::private::create_dir(&state).unwrap();
        let path = state.join("ledger.json");
        let ledger = letter();
        save(&path, &ledger).unwrap();
        assert_eq!(recover(&path).unwrap(), ledger);
        std::fs::write(&path, b"private damaged bytes").unwrap();
        assert_eq!(recover(&path).unwrap_err().code(), "ledger_unavailable");
        assert_eq!(std::fs::read(&path).unwrap(), b"private damaged bytes");
        // A missing parent is not implicitly bootstrapped by a transaction.
        let missing = root.path().join("absent/ledger.json");
        let error = save(&missing, &Ledger::default()).unwrap_err();
        assert!(!error.uncertain());
        assert!(!missing.parent().unwrap().exists());
        assert_eq!(error.diagnostic()["phase"], "before_replace");
        assert!(
            !error
                .diagnostic()
                .to_string()
                .contains(&root.path().display().to_string())
        );
    }

    #[test]
    fn startup_bootstraps_private_sibling_state_and_preserves_refused_or_corrupt_bytes() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        hide_platform::fs::private::create_dir(&home).unwrap();
        let state = root.path().join("new/state");
        let path = state.join("ledger.json");
        assert_eq!(recover(&path).unwrap(), Ledger::default());
        assert!(hide_platform::fs::private::is_private(&state).unwrap());
        assert!(hide_platform::fs::private::is_private(&path).unwrap());
        assert!(home.is_dir());
        std::fs::write(&path, b"private corruption").unwrap();
        assert_eq!(recover(&path).unwrap_err().code(), "ledger_unavailable");
        assert_eq!(std::fs::read(&path).unwrap(), b"private corruption");
        #[cfg(unix)]
        {
            let link = root.path().join("linked-state");
            std::os::unix::fs::symlink(&state, &link).unwrap();
            assert_eq!(
                recover(&link.join("ledger.json")).unwrap_err().code(),
                "ledger_unavailable"
            );
            assert_eq!(std::fs::read(&path).unwrap(), b"private corruption");
            assert!(
                std::fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
    }

    #[test]
    fn durable_ledger_is_private_and_corruption_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("ledger.json");
        save(&path, &Ledger::default()).unwrap();
        assert!(hide_platform::fs::private::is_private(&path).unwrap());
        assert_eq!(load(&path).unwrap(), Ledger::default());
        std::fs::write(&path, b"broken ledger").unwrap();
        assert_eq!(load(&path).unwrap_err(), "ledger_unavailable");
        assert_eq!(std::fs::read(&path).unwrap(), b"broken ledger");
    }
}
