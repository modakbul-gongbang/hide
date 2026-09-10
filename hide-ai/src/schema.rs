use serde_json::Value;

use crate::AiError;

/// Re-validates a provider answer against the feature's schema. The provider
/// may claim to enforce the schema; the router does not take its word for it.
pub(crate) fn validate(schema: &Value, instance: &Value) -> Result<(), AiError> {
    let validator = jsonschema::validator_for(schema)
        .map_err(|error| AiError::InvalidOutput(format!("schema_unusable:{error}")))?;
    match validator.iter_errors(instance).next() {
        None => Ok(()),
        Some(error) => Err(AiError::InvalidOutput(format!(
            "schema_mismatch:{}",
            error.instance_path()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["summary", "attention"],
            "properties": {
                "summary": {"type": "string"},
                "attention": {"type": "string", "enum": ["question", "none"]}
            }
        })
    }

    #[test]
    fn a_conforming_answer_passes() {
        assert_eq!(
            validate(&schema(), &json!({"summary": "x", "attention": "none"})),
            Ok(())
        );
    }

    #[test]
    fn a_missing_field_or_bad_enum_is_invalid_output_naming_the_path() {
        let missing = validate(&schema(), &json!({"summary": "x"})).unwrap_err();
        assert!(matches!(missing, AiError::InvalidOutput(_)), "{missing:?}");
        let bad =
            validate(&schema(), &json!({"summary": "x", "attention": "approval"})).unwrap_err();
        assert!(bad.to_string().contains("/attention"), "{bad}");
    }
}
