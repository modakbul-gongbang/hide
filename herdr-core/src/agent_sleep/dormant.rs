//! Durable sleeping-session identity, separate from every execution/pane.
use super::*;
use crate::recent_closed::ClosedContext;

/// A finite archive, independent of per-tab geometry or tree-close budgets.
pub const MAX_DORMANT_RECORDS: usize = 256;
/// Manual and automatic dormant transitions share this global admission cap.
pub const MAX_DORMANT_IN_FLIGHT: usize = 4;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SleepId(String);

impl SleepId {
    pub(crate) fn new() -> Result<Self, &'static str> {
        // This is an intent identity, never an address or a credential.
        // The saved identity is reused on every recovery and wake attempt.
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let sequence = SEQUENCE
            .try_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |value| value.checked_add(1),
            )
            .map_err(|_| "Sleeping-session identity capacity is exhausted")?;
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "System time cannot establish a sleeping-session identity")?
            .as_nanos();
        Ok(Self(format!("sleep-{time:x}-{sequence:x}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SleepId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        let valid_suffix = value.strip_prefix("sleep-").is_some_and(|suffix| {
            let mut parts = suffix.split('-');
            let valid_part = |part: Option<&str>| {
                part.is_some_and(|part| {
                    !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            };
            valid_part(parts.next()) && valid_part(parts.next()) && parts.next().is_none()
        });
        if !(8..=80).contains(&value.len()) || !valid_suffix {
            return Err(serde::de::Error::custom(
                "invalid sleeping-session identity",
            ));
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DormantPhase {
    SavingClose,
    SavingCloseReady,
    Closing,
    CloseUnknown,
    Sleeping,
    SavingWake,
    Creating,
    SavingStart,
    Starting,
    WakeUnknown,
    Failed,
}

impl DormantPhase {
    pub fn in_flight(self) -> bool {
        matches!(
            self,
            Self::SavingClose
                | Self::SavingCloseReady
                | Self::Closing
                | Self::SavingWake
                | Self::Creating
                | Self::SavingStart
                | Self::Starting
                | Self::CloseUnknown
                | Self::WakeUnknown
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DormantRecord {
    pub phase: DormantPhase,
    /// A save completion may act only on exactly this revision.
    pub revision: u64,
    pub node_id: String,
    pub connection_generation: u64,
    pub old_pane_id: String,
    pub old_state_change_seq: Option<u64>,
    pub kind: String,
    pub native_session_id: String,
    /// The exact reported locator admitted before closing the original pane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_reference: Option<crate::sidebar::SessionAgentSessionPayload>,
    /// Provider/native reference proof, not an old terminal attestation.
    pub label_owner: String,
    pub identity_label: String,
    pub cwd: String,
    pub context: ClosedContext,
    #[serde(default)]
    pub close_key: Option<String>,
    /// Set only by the close operation's authoritative topology confirmation.
    #[serde(default)]
    pub closed: bool,
    /// Only a pane carrying this sleep intent may be adopted on recovery.
    #[serde(default)]
    pub wake_pane_id: Option<String>,
    #[serde(default)]
    pub wake_tab_id: Option<String>,
    pub since_unix_ms: u64,
    #[serde(default)]
    pub transition_started_unix_ms: u64,
    #[serde(default)]
    pub reason: Option<String>,
}

impl DormantRecord {
    /// Bounds include topology retained for exact intent-marker recovery.
    /// An over-bound capture is refused, never truncated into another owner.
    pub fn validate(&self) -> Result<(), &'static str> {
        self.validate_context()?;
        if hide_agent_adapter::canonical_kind(&self.kind) == "pi"
            && self.source_reference.as_ref().is_none_or(|reference| {
                reference.value.len() > 4096
                    || hide_session::label_reference_token(
                        &self.kind,
                        &reference.kind,
                        &reference.value,
                    )
                    .is_none()
            })
        {
            return Err("Sleeping-session source reference is unconfirmed");
        }
        if self.old_state_change_seq.is_none() {
            return Err("Sleeping-session execution identity is unconfirmed");
        }
        if hide_session::label_reference_token(&self.kind, "id", &self.native_session_id).as_deref()
            != Some(self.label_owner.as_str())
        {
            return Err("Sleeping-session native identity is unconfirmed");
        }
        Ok(())
    }

    fn validate_context(&self) -> Result<(), &'static str> {
        let scalar = [
            self.node_id.as_str(),
            self.old_pane_id.as_str(),
            self.kind.as_str(),
            self.native_session_id.as_str(),
            self.label_owner.as_str(),
            self.identity_label.as_str(),
            self.cwd.as_str(),
            self.context.workspace_id.as_str(),
            self.context.workspace_label.as_str(),
            self.context.checkout_id.as_str(),
            self.context.checkout_path.as_str(),
            self.context.tab_id.as_str(),
            self.context.tab_label.as_str(),
        ];
        if scalar.iter().any(|value| {
            value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control)
        }) || self.revision == u64::MAX
            || self
                .close_key
                .as_ref()
                .is_some_and(|value| value.len() > 128)
            || self
                .wake_pane_id
                .as_ref()
                .is_some_and(|value| value.len() > 128)
            || self
                .wake_tab_id
                .as_ref()
                .is_some_and(|value| value.len() > 128)
            || self.reason.as_ref().is_some_and(|value| value.len() > 4096)
        {
            return Err("Sleeping-session identity or context is invalid or exceeds its capacity");
        }
        for ids in [
            &self.context.workspace_ids_before_close,
            &self.context.tab_ids_before_close,
            &self.context.pane_ids_before_close,
        ] {
            if ids.len() > 256
                || ids
                    .iter()
                    .any(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
            {
                return Err("Sleeping-session recovery topology exceeds its capacity");
            }
        }
        Ok(())
    }

    pub(crate) fn after_load(&mut self) {
        self.phase = match self.phase {
            DormantPhase::SavingClose | DormantPhase::SavingCloseReady | DormantPhase::Closing => {
                DormantPhase::CloseUnknown
            }
            DormantPhase::SavingWake
            | DormantPhase::Creating
            | DormantPhase::SavingStart
            | DormantPhase::Starting => DormantPhase::WakeUnknown,
            phase => phase,
        };
        // Unknown work is inspected, never resent after restart.
    }

    pub(crate) fn transition(&mut self, phase: DormantPhase) -> Result<(), &'static str> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or("Sleeping-session revision capacity is exhausted")?;
        self.phase = phase;
        self.reason = None;
        Ok(())
    }

    pub(crate) fn snapshot(&self, id: &SleepId) -> SleepingSessionSnapshot {
        SleepingSessionSnapshot {
            sleep_id: id.clone(),
            node_id: self.node_id.clone(),
            checkout_path: self.context.checkout_path.clone(),
            kind: self.kind.clone(),
            identity_label: self.identity_label.clone(),
            phase: self.phase,
            since_unix_ms: self.since_unix_ms,
            reason: self.reason.clone(),
            // Historical parent metadata never creates a live relationship.
            group: crate::agent_state::dormant_group().name(),
            wake_available: self.closed
                && matches!(self.phase, DormantPhase::Sleeping | DormantPhase::Failed)
                && hide_agent_adapter::adapter(&self.kind)
                    .is_some_and(|adapter| adapter.resume.is_some()),
            checking: false,
        }
    }
}

pub(super) fn deserialize_records<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<SleepId, DormantRecord>, D::Error> {
    struct Records;
    impl<'de> serde::de::Visitor<'de> for Records {
        type Value = BTreeMap<SleepId, DormantRecord>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a bounded sleeping-session archive")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut records = BTreeMap::new();
            while let Some((id, record)) = map.next_entry::<SleepId, DormantRecord>()? {
                if records.len() >= MAX_DORMANT_RECORDS || records.contains_key(&id) {
                    return Err(serde::de::Error::custom(
                        "sleeping-session archive capacity or duplicate identity",
                    ));
                }
                record
                    .validate_context()
                    .map_err(serde::de::Error::custom)?;
                records.insert(id, record);
            }
            Ok(records)
        }
    }
    deserializer.deserialize_map(Records)
}

impl AgentSleepStore {
    /// The same old execution cannot acquire a second intent while its first
    /// close is pending. Returned identities are durable session identities.
    pub fn admit_dormant(&mut self, record: DormantRecord) -> Result<SleepId, &'static str> {
        record.validate()?;
        if let Some((id, _)) = self.dormant.iter().find(|(_, existing)| {
            existing.node_id == record.node_id
                && existing.connection_generation == record.connection_generation
                && existing.old_pane_id == record.old_pane_id
                && existing.native_session_id == record.native_session_id
                && matches!(
                    existing.phase,
                    DormantPhase::SavingClose
                        | DormantPhase::SavingCloseReady
                        | DormantPhase::Closing
                )
        }) {
            return Ok(id.clone());
        }
        if self.dormant.len() >= MAX_DORMANT_RECORDS {
            return Err("The sleeping-session archive is full; resolve an earlier record first");
        }
        self.admit_dormant_transition()?;
        let id = SleepId::new()?;
        if self.dormant.contains_key(&id) {
            return Err("Sleeping-session identity collided; no pane was closed");
        }
        self.dormant.insert(id.clone(), record);
        Ok(id)
    }

    pub fn admit_dormant_transition(&self) -> Result<(), &'static str> {
        if self
            .dormant
            .values()
            .filter(|record| record.phase.in_flight())
            .count()
            >= MAX_DORMANT_IN_FLIGHT
        {
            Err("Resolve a pending sleeping-session operation before starting another")
        } else {
            Ok(())
        }
    }

    /// A coalesced write acknowledges only records it actually wrote. An old
    /// revision, phase or connection can never authorize an external effect.
    pub fn dormant_saved(
        &self,
        saved: &Self,
        node: &str,
        generation: u64,
    ) -> Vec<(SleepId, DormantPhase)> {
        saved
            .dormant
            .iter()
            .filter_map(|(id, written)| {
                let current = self.dormant.get(id)?;
                (current == written
                    && current.node_id == node
                    && current.connection_generation == generation
                    && matches!(
                        current.phase,
                        DormantPhase::SavingClose
                            | DormantPhase::SavingCloseReady
                            | DormantPhase::SavingWake
                            | DormantPhase::SavingStart
                    ))
                .then(|| (id.clone(), current.phase))
            })
            .collect()
    }

    pub fn dormant_snapshots(&self) -> Vec<SleepingSessionSnapshot> {
        self.dormant
            .iter()
            // While saving/closing, the original live row still exists.
            // An unknown close needs its own explicit status action.
            .filter(|(_, record)| {
                record.closed
                    || matches!(
                        record.phase,
                        DormantPhase::CloseUnknown | DormantPhase::WakeUnknown
                    )
            })
            .map(|(id, record)| record.snapshot(id))
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SleepingSessionSnapshot {
    pub sleep_id: SleepId,
    pub node_id: String,
    pub checkout_path: String,
    pub kind: String,
    pub identity_label: String,
    pub phase: DormantPhase,
    pub since_unix_ms: u64,
    pub reason: Option<String>,
    pub group: &'static str,
    pub wake_available: bool,
    pub checking: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pane: &str) -> DormantRecord {
        DormantRecord {
            source_reference: None,
            phase: DormantPhase::SavingClose,
            revision: 1,
            node_id: "fixture-node".into(),
            connection_generation: 7,
            old_pane_id: pane.into(),
            old_state_change_seq: Some(3),
            kind: "claude".into(),
            native_session_id: "native-one".into(),
            label_owner: hide_session::label_reference_token("claude", "id", "native-one").unwrap(),
            identity_label: "Parser task".into(),
            cwd: "/fixture/repo".into(),
            context: ClosedContext {
                workspace_id: "w1".into(),
                workspace_label: "Fixture".into(),
                workspace_ids_before_close: vec!["w1".into()],
                tab_ids_before_close: vec!["t1".into()],
                pane_ids_before_close: vec![pane.into()],
                checkout_id: "checkout".into(),
                checkout_path: "/fixture/repo".into(),
                tab_id: "t1".into(),
                tab_label: "Tab".into(),
                tab_index: 0,
                agent_area: None,
                replacement_shell: false,
            },
            close_key: None,
            closed: false,
            wake_pane_id: None,
            wake_tab_id: None,
            since_unix_ms: 1,
            transition_started_unix_ms: 1,
            reason: None,
        }
    }

    #[test]
    fn duplicate_sleep_converges_and_save_receipts_reject_changed_work() {
        let mut store = AgentSleepStore::default();
        let id = store.admit_dormant(record("p1")).unwrap();
        assert_eq!(store.admit_dormant(record("p1")).unwrap(), id);
        assert_eq!(store.dormant.len(), 1);
        assert_ne!(id.as_str(), "p1");
        let written = store.clone();
        assert_eq!(
            store.dormant_saved(&written, "fixture-node", 7),
            vec![(id.clone(), DormantPhase::SavingClose)]
        );
        assert!(store.dormant_saved(&written, "other-node", 7).is_empty());
        assert!(store.dormant_saved(&written, "fixture-node", 8).is_empty());
        store
            .dormant
            .get_mut(&id)
            .unwrap()
            .transition(DormantPhase::Closing)
            .unwrap();
        assert!(store.dormant_saved(&written, "fixture-node", 7).is_empty());
        // Equality covers immutable native proof as well as the revision.
        store = written.clone();
        store.dormant.get_mut(&id).unwrap().native_session_id = "replacement".into();
        assert!(store.dormant_saved(&written, "fixture-node", 7).is_empty());
    }

    #[test]
    fn manual_and_automatic_admission_share_four_operations_and_a_finite_archive() {
        let mut store = AgentSleepStore::default();
        for pane in ["p1", "p2", "p3", "p4"] {
            store.admit_dormant(record(pane)).unwrap();
        }
        assert!(store.admit_dormant(record("p5")).is_err());
        // Unknown external effects retain the admission instead of allowing
        // an unbounded succession of uncertain creates.
        for entry in store.dormant.values_mut() {
            entry.phase = DormantPhase::WakeUnknown;
        }
        assert!(store.admit_dormant(record("p5")).is_err());
        for entry in store.dormant.values_mut() {
            entry.phase = DormantPhase::Sleeping;
            entry.closed = true;
        }
        for index in 4..256 {
            let id = store
                .admit_dormant(record(&format!("p{}", index + 1)))
                .unwrap();
            let entry = store.dormant.get_mut(&id).unwrap();
            entry.phase = DormantPhase::Sleeping;
            entry.closed = true;
        }
        assert_eq!(store.dormant.len(), 256);
        assert!(store.admit_dormant(record("p257")).is_err());
    }

    #[test]
    fn restart_never_resends_an_uncertain_close_or_start_and_preserves_legacy_storage() {
        let mut store = AgentSleepStore::default();
        let id = store.admit_dormant(record("p1")).unwrap();
        for (phase, recovered) in [
            (DormantPhase::SavingClose, DormantPhase::CloseUnknown),
            (DormantPhase::SavingCloseReady, DormantPhase::CloseUnknown),
            (DormantPhase::Closing, DormantPhase::CloseUnknown),
            (DormantPhase::SavingWake, DormantPhase::WakeUnknown),
            (DormantPhase::Creating, DormantPhase::WakeUnknown),
            (DormantPhase::SavingStart, DormantPhase::WakeUnknown),
            (DormantPhase::Starting, DormantPhase::WakeUnknown),
        ] {
            store.dormant.get_mut(&id).unwrap().phase = phase;
            let bytes = serde_json::to_vec(&store).unwrap();
            let mut loaded: AgentSleepStore = serde_json::from_slice(&bytes).unwrap();
            loaded.after_load();
            assert_eq!(loaded.dormant[&id].phase, recovered);
            assert!(loaded.dormant_saved(&store, "fixture-node", 7).is_empty());
            assert!(!loaded.dormant_snapshots()[0].wake_available);
            assert_eq!(loaded.dormant_snapshots()[0].group, "seen");
        }
        let old: AgentSleepStore = serde_json::from_str(r#"{"stamps":{},"records":{}}"#).unwrap();
        assert!(old.dormant.is_empty());
    }

    #[test]
    fn retained_sessions_offer_wake_only_for_this_builds_resume_capabilities() {
        let mut store = AgentSleepStore::default();
        let id = store.admit_dormant(record("p1")).unwrap();
        let entry = store.dormant.get_mut(&id).unwrap();
        entry.phase = DormantPhase::Sleeping;
        entry.closed = true;
        assert!(store.dormant_snapshots()[0].wake_available);
        store.dormant.get_mut(&id).unwrap().kind = "pi".into();
        assert!(store.dormant_snapshots()[0].wake_available);
        for kind in ["omp", "grok", "cursor", "opencode", "unknown"] {
            store.dormant.get_mut(&id).unwrap().kind = kind.into();
            assert!(!store.dormant_snapshots()[0].wake_available, "{kind}");
        }
        assert_eq!(store.dormant.len(), 1);
    }

    #[test]
    fn malformed_identity_duplicate_keys_and_overbound_context_are_refused() {
        for value in ["p1", "sleep-", "sleep-gg-1", "sleep-1-2-3", "sleep-1-/1"] {
            assert!(serde_json::from_value::<SleepId>(serde_json::json!(value)).is_err());
        }
        let record = record("p1");
        let encoded = serde_json::to_string(&record).unwrap();
        let duplicate = [
            r#"{"dormant":{"sleep-1-1":"#,
            &encoded,
            r#","sleep-1-1":"#,
            &encoded,
            "}}",
        ]
        .concat();
        assert!(serde_json::from_str::<AgentSleepStore>(&duplicate).is_err());
        let mut too_many = record;
        too_many.context.tab_ids_before_close = vec!["t".into(); 257];
        let invalid = serde_json::json!({"dormant":{"sleep-1-1":too_many}});
        assert!(serde_json::from_value::<AgentSleepStore>(invalid).is_err());
    }
}
