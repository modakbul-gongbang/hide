//! Generated Herdr API contract boundary.
//!
//! `contracts/herdr-api.schema.json` is copied from Herdr's generated schema.
//! The build script derives constants from that artifact so production code
//! never carries an independently maintained protocol number.

pub const HERDR_API_SCHEMA_JSON: &str = include_str!("../../contracts/herdr-api.schema.json");

include!(concat!(env!("OUT_DIR"), "/herdr_contract.rs"));

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

// These types are generated from the pinned external schema. Their names,
// defaults, and value layout follow that contract rather than local style.
#[allow(
    dead_code,
    clippy::derivable_impls,
    clippy::enum_variant_names,
    clippy::large_enum_variant
)]
pub(crate) mod wire {
    pub mod request {
        include!(concat!(env!("OUT_DIR"), "/herdr_request.rs"));
    }
    pub mod success_response {
        include!(concat!(env!("OUT_DIR"), "/herdr_success_response.rs"));
    }
    pub mod event {
        include!(concat!(env!("OUT_DIR"), "/herdr_event.rs"));
    }
    pub mod subscription_event {
        include!(concat!(env!("OUT_DIR"), "/herdr_subscription_event.rs"));
    }
    pub mod error_response {
        include!(concat!(env!("OUT_DIR"), "/herdr_error_response.rs"));
    }
}
