//! Paired phones and the pairing code (PRD D-04, D-19).
//!
//! One pairing code lives at a time, in memory only: five minutes, one use,
//! and a new code voids the old one. A paired phone holds a credential of its
//! own; `phones.json` keeps only its SHA-256, the phone's name, when it was
//! last seen, and its push subscription. At most four phones; a phone not seen
//! for seven days is revoked, and a revoke removes the credential and the
//! subscription in one write.

use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use subtle::ConstantTimeEq;

use super::store::{self, Notifications, PhoneRecord, PhonesFile, PushSubscription};

pub const MAX_PHONES: usize = 4;
pub const CODE_TTL_MS: u64 = 5 * 60 * 1000;
pub const INACTIVE_REVOKE_MS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PairRefusal {
    /// Expired, already used, replaced by a newer code, or never issued.
    CodeExpired,
    /// Four phones are paired already.
    PhoneLimit,
}

impl PairRefusal {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::CodeExpired => "code_expired",
            Self::PhoneLimit => "phone_limit",
        }
    }
}

struct PairingCode {
    code: String,
    expires_at_ms: u64,
}

pub struct Phones {
    path: PathBuf,
    file: PhonesFile,
    code: Option<PairingCode>,
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0_u8; N];
    getrandom::getrandom(&mut bytes).expect("getrandom");
    bytes
}

pub fn credential_hash(credential: &str) -> String {
    hex::encode(ring::digest::digest(
        &ring::digest::SHA256,
        credential.as_bytes(),
    ))
}

/// A readable name from the phone's user agent; the PWA cannot read a model.
pub fn name_from_user_agent(user_agent: &str) -> &'static str {
    if user_agent.contains("iPad") {
        "iPad"
    } else if user_agent.contains("iPhone") {
        "iPhone"
    } else if user_agent.contains("Android") {
        "Android"
    } else {
        "Phone"
    }
}

impl Phones {
    pub fn load(path: PathBuf) -> Self {
        let file: PhonesFile = store::read(&path);
        Self {
            path,
            file,
            code: None,
        }
    }

    fn save(&self) -> bool {
        store::write_logged(&self.path, &self.file)
    }

    pub fn list(&self) -> &[PhoneRecord] {
        &self.file.phones
    }

    pub fn is_empty(&self) -> bool {
        self.file.phones.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&PhoneRecord> {
        self.file.phones.iter().find(|phone| phone.id == id)
    }

    /// A fresh code, voiding any earlier one.
    pub fn new_code(&mut self, now_ms: u64) -> (String, u64) {
        let code = URL_SAFE_NO_PAD.encode(random_bytes::<16>());
        let expires_at_ms = now_ms + CODE_TTL_MS;
        self.code = Some(PairingCode {
            code: code.clone(),
            expires_at_ms,
        });
        (code, expires_at_ms)
    }

    /// The live code and when it expires, if one was issued and is unused.
    pub fn code(&self) -> Option<(&str, u64)> {
        self.code
            .as_ref()
            .map(|code| (code.code.as_str(), code.expires_at_ms))
    }

    pub fn clear_code(&mut self) {
        self.code = None;
    }

    /// Pairs a phone with the live code. The code is spent only by a pairing
    /// that succeeds; the phone limit leaves it for after a revoke. Two
    /// phones racing on one code are serialized by the caller's lock, so the
    /// second finds it spent.
    pub fn pair(
        &mut self,
        offered: &str,
        user_agent: &str,
        now_ms: u64,
    ) -> Result<(PhoneRecord, String), PairRefusal> {
        let valid = self.code.as_ref().is_some_and(|code| {
            now_ms < code.expires_at_ms
                && code.code.len() == offered.len()
                && bool::from(code.code.as_bytes().ct_eq(offered.as_bytes()))
        });
        if !valid {
            return Err(PairRefusal::CodeExpired);
        }
        if self.file.phones.len() >= MAX_PHONES {
            return Err(PairRefusal::PhoneLimit);
        }
        self.code = None;
        let credential = hex::encode(random_bytes::<32>());
        let base = name_from_user_agent(user_agent);
        let taken = |name: &str| self.file.phones.iter().any(|phone| phone.name == name);
        let name = if taken(base) {
            (2..)
                .map(|n| format!("{base} {n}"))
                .find(|name| !taken(name))
                .expect("an unused name")
        } else {
            base.to_owned()
        };
        let record = PhoneRecord {
            id: hex::encode(random_bytes::<8>()),
            name,
            credential_sha256: credential_hash(&credential),
            paired_at_ms: now_ms,
            last_seen_ms: now_ms,
            notifications: Notifications::Unasked,
            push: None,
        };
        self.file.phones.push(record.clone());
        self.save();
        Ok((record, credential))
    }

    /// The phone a credential belongs to. Every record is compared, in
    /// constant time per record, so the answer's timing does not say which.
    pub fn authenticate(&self, credential: &str) -> Option<&PhoneRecord> {
        let offered = credential_hash(credential);
        let mut found = None;
        for phone in &self.file.phones {
            if bool::from(phone.credential_sha256.as_bytes().ct_eq(offered.as_bytes())) {
                found = Some(phone);
            }
        }
        found
    }

    pub fn touch(&mut self, id: &str, now_ms: u64) {
        if let Some(phone) = self.file.phones.iter_mut().find(|phone| phone.id == id) {
            phone.last_seen_ms = now_ms;
            self.save();
        }
    }

    /// Revokes one phone; `None` when it was already gone, so a second
    /// revoke is a quiet no-op.
    pub fn revoke(&mut self, id: &str) -> Option<PhoneRecord> {
        let index = self.file.phones.iter().position(|phone| phone.id == id)?;
        let removed = self.file.phones.remove(index);
        self.save();
        Some(removed)
    }

    /// Revokes every phone not seen for seven days, except the ones in
    /// `connected` (a phone on a live connection is being seen now).
    pub fn sweep(&mut self, now_ms: u64, connected: &[String]) -> Vec<PhoneRecord> {
        let (stale, kept): (Vec<_>, Vec<_>) = self.file.phones.drain(..).partition(|phone| {
            !connected.contains(&phone.id)
                && now_ms.saturating_sub(phone.last_seen_ms) >= INACTIVE_REVOKE_MS
        });
        self.file.phones = kept;
        if !stale.is_empty() {
            self.save();
        }
        stale
    }

    pub fn set_subscription(&mut self, id: &str, push: Option<PushSubscription>) -> bool {
        let Some(phone) = self.file.phones.iter_mut().find(|phone| phone.id == id) else {
            return false;
        };
        let notifications = if push.is_some() {
            Notifications::On
        } else {
            phone.notifications
        };
        if phone.push == push && phone.notifications == notifications {
            return false;
        }
        phone.push = push;
        phone.notifications = notifications;
        self.save();
        true
    }

    pub fn set_notifications(&mut self, id: &str, notifications: Notifications) -> bool {
        let Some(phone) = self.file.phones.iter_mut().find(|phone| phone.id == id) else {
            return false;
        };
        let push = if notifications == Notifications::Off {
            None
        } else {
            phone.push.clone()
        };
        if phone.notifications == notifications && phone.push == push {
            return false;
        }
        phone.notifications = notifications;
        phone.push = push;
        self.save();
        true
    }

    /// Drops a subscription the push service called gone; the phone
    /// registers a new one on its next connection.
    pub fn drop_subscription(&mut self, id: &str, endpoint: &str) -> bool {
        let Some(phone) = self.file.phones.iter_mut().find(|phone| {
            phone.id == id
                && phone
                    .push
                    .as_ref()
                    .is_some_and(|push| push.endpoint == endpoint)
        }) else {
            return false;
        };
        phone.push = None;
        self.save();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)";

    fn phones() -> (tempfile::TempDir, Phones) {
        let dir = tempfile::tempdir().unwrap();
        let phones = Phones::load(store::phones_path(dir.path()));
        (dir, phones)
    }

    #[test]
    fn a_code_pairs_once_and_a_new_code_voids_the_old() {
        let (_dir, mut phones) = phones();
        let (old, _) = phones.new_code(1_000);
        let (code, expires) = phones.new_code(2_000);
        assert_eq!(expires, 2_000 + CODE_TTL_MS);
        assert_eq!(
            phones.pair(&old, UA, 3_000).unwrap_err(),
            PairRefusal::CodeExpired
        );
        let (record, credential) = phones.pair(&code, UA, 3_000).unwrap();
        assert_eq!(record.name, "iPhone");
        // The second phone on the same code finds it spent.
        assert_eq!(
            phones.pair(&code, UA, 3_001).unwrap_err(),
            PairRefusal::CodeExpired
        );
        assert_eq!(
            phones
                .authenticate(&credential)
                .map(|phone| phone.id.clone()),
            Some(record.id.clone())
        );
        assert!(phones.authenticate("0".repeat(64).as_str()).is_none());
        // Only the hash is on disk.
        let text = std::fs::read_to_string(&phones.path).unwrap();
        assert!(!text.contains(&credential));
        assert!(!text.contains(&code));
    }

    #[test]
    fn a_code_expires_after_five_minutes() {
        let (_dir, mut phones) = phones();
        let (code, expires) = phones.new_code(0);
        assert_eq!(
            phones.pair(&code, UA, expires).unwrap_err(),
            PairRefusal::CodeExpired
        );
        let (code, _) = phones.new_code(0);
        assert!(phones.pair(&code, UA, CODE_TTL_MS - 1).is_ok());
    }

    #[test]
    fn a_fifth_phone_is_refused_and_keeps_the_code() {
        let (_dir, mut phones) = phones();
        for _ in 0..MAX_PHONES {
            let (code, _) = phones.new_code(0);
            phones.pair(&code, UA, 1).unwrap();
        }
        let names: Vec<_> = phones
            .list()
            .iter()
            .map(|phone| phone.name.clone())
            .collect();
        assert_eq!(names, ["iPhone", "iPhone 2", "iPhone 3", "iPhone 4"]);
        let (code, _) = phones.new_code(0);
        assert_eq!(
            phones.pair(&code, UA, 1).unwrap_err(),
            PairRefusal::PhoneLimit
        );
        let first = phones.list()[0].id.clone();
        phones.revoke(&first);
        assert!(
            phones.pair(&code, UA, 2).is_ok(),
            "the code survives the refusal"
        );
    }

    #[test]
    fn revoke_is_idempotent_and_takes_the_subscription_with_it() {
        let (dir, mut phones) = phones();
        let (code, _) = phones.new_code(0);
        let (record, credential) = phones.pair(&code, UA, 1).unwrap();
        phones.set_subscription(
            &record.id,
            Some(PushSubscription {
                endpoint: "https://web.push.apple.com/x".into(),
                p256dh: "k".into(),
                auth: "a".into(),
            }),
        );
        assert!(phones.revoke(&record.id).is_some());
        assert!(phones.revoke(&record.id).is_none());
        assert!(phones.authenticate(&credential).is_none());
        let reread = Phones::load(store::phones_path(dir.path()));
        assert!(reread.is_empty());
        let text = std::fs::read_to_string(store::phones_path(dir.path())).unwrap();
        assert!(!text.contains("web.push.apple.com"));
    }

    #[test]
    fn seven_days_unseen_revokes_unless_connected() {
        let (_dir, mut phones) = phones();
        let (code, _) = phones.new_code(0);
        let (old, _) = phones.pair(&code, UA, 0).unwrap();
        let (code, _) = phones.new_code(0);
        let (live, _) = phones.pair(&code, UA, 0).unwrap();
        let (code, _) = phones.new_code(0);
        let (recent, _) = phones.pair(&code, UA, 0).unwrap();
        phones.touch(&recent.id, INACTIVE_REVOKE_MS - 10);
        let revoked = phones.sweep(INACTIVE_REVOKE_MS, std::slice::from_ref(&live.id));
        assert_eq!(
            revoked.iter().map(|phone| &phone.id).collect::<Vec<_>>(),
            [&old.id]
        );
        let left: Vec<_> = phones.list().iter().map(|phone| phone.id.clone()).collect();
        assert_eq!(left, [live.id, recent.id]);
    }
}
