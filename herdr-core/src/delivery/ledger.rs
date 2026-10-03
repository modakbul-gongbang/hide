use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::watch::Watch;
use super::{Actor, BODY_LIMIT, FILE_LIMIT, LETTER_LIMIT, OPEN_LIMIT, RETENTION_MS, WATCH_LIMIT};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Pending,
    Delivered,
    Acknowledged,
    Cancelled,
    Expired,
    Undelivered,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Letter {
    pub id: String,
    pub intent: String,
    pub sender: Actor,
    pub recipient: Actor,
    pub kind: String,
    pub body: String,
    pub state: State,
    pub waiting_answer: bool,
    pub reply_to: Option<String>,
    pub created_at_unix_ms: u64,
    pub finished_at_unix_ms: Option<u64>,
    pub bell_errors: u8,
    pub bell_sent: bool,
}

impl Letter {
    pub fn open(&self) -> bool {
        self.state == State::Pending || self.waiting_answer
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    pub version: u32,
    pub next_id: u64,
    pub letters: Vec<Letter>,
    pub watches: Vec<Watch>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            version: 1,
            next_id: 1,
            letters: Vec::new(),
            watches: Vec::new(),
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
                || !super::valid_key(&letter.intent)
                || !letter.sender.valid()
                || !letter.recipient.valid()
                || !matches!(letter.kind.as_str(), "request" | "reply" | "watch")
                || letter.sender.device_id != "local"
                || letter.recipient.device_id != "local"
                || letter.body.len() > BODY_LIMIT
                || letter.bell_errors > 3
                || (!letter.open() && letter.finished_at_unix_ms.is_none())
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
            if !watches.insert(&watch.id) || !watch.valid() {
                return Err("ledger_unavailable".into());
            }
        }
        for id in ids.into_iter().chain(watches) {
            let sequence = id
                .split_once('-')
                .and_then(|(_, value)| value.parse::<u64>().ok())
                .ok_or("ledger_unavailable")?;
            if sequence >= self.next_id {
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
            if letter.state == State::Pending
                && now.saturating_sub(letter.created_at_unix_ms) >= super::DELIVERY_EXPIRY_MS
            {
                letter.state = State::Undelivered;
                letter.waiting_answer = false;
                letter.finished_at_unix_ms = Some(now);
                changed = true;
            }
        }
        changed
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

pub fn save(path: &Path, ledger: &Ledger) -> Result<(), String> {
    let bytes = ledger.bytes()?;
    let parent = path.parent().ok_or("ledger_unavailable")?;
    hide_platform::fs::private::create_dir_all(parent).map_err(|_| "ledger_unavailable")?;
    hide_platform::fs::atomic::write_file(path, &bytes, hide_platform::fs::Access::Private)
        .map_err(|_| "ledger_unavailable")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
