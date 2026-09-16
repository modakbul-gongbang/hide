//! Compatibility view of the canonical Herdr contract for core callers.
//!
//! The generated contract and wire types live in `hide-herdr-client`. Core
//! keeps this tiny module so its public schema assertions and domain protocol
//! checks remain in the core crate while importing one source of truth.

pub const HERDR_API_SCHEMA_JSON: &str = hide_herdr_client::HERDR_API_SCHEMA_JSON;
pub const HERDR_PROTOCOL_REVISION: u32 = hide_herdr_client::HERDR_PROTOCOL_REVISION as u32;
pub const HERDR_API_SCHEMA_VERSION: u32 = hide_herdr_client::HERDR_API_SCHEMA_VERSION;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_schema_is_the_generated_contract() {
        let schema: serde_json::Value = serde_json::from_str(HERDR_API_SCHEMA_JSON).unwrap();
        assert_eq!(schema["protocol"], HERDR_PROTOCOL_REVISION);
        assert_eq!(schema["schema_version"], HERDR_API_SCHEMA_VERSION);
    }

    /// A protocol number that matches is not enough: the replica refuses a
    /// snapshot without the fields it reads, so the pinned Herdr must promise
    /// every one of them. This is the check the schema gate cannot make.
    #[test]
    fn the_pinned_herdr_promises_every_snapshot_field_the_replica_reads() {
        let schema: serde_json::Value = serde_json::from_str(HERDR_API_SCHEMA_JSON).unwrap();
        let required =
            schema["schemas"]["success_response"]["$defs"]["SessionSnapshot"]["required"]
                .as_array()
                .expect("SessionSnapshot.required")
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>();
        let missing = crate::session_sync::SNAPSHOT_FIELDS_THE_REPLICA_READS
            .iter()
            .filter(|field| !required.contains(field))
            .collect::<Vec<_>>();
        assert!(
            missing.is_empty(),
            "the pinned Herdr's session.snapshot does not promise {missing:?}; \
             hide cannot run on this Herdr until the core stops reading them \
             or a Herdr that carries them is pinned"
        );
    }
}
