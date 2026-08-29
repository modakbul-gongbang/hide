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
}
