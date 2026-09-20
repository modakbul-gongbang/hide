use hide_ai::{AiRequest, RequestId};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fmt::{Display, Formatter};
use std::time::Duration;

use crate::{ANALYSIS_INPUT_LIMIT_BYTES, Candidate, CandidateKind, CandidateRelation, redact};

/// Pinned upstream semantics reviewed for this adapter.
///
/// The native app does not embed Python or Node. This is a native port of the
/// Mem0 OSS v2.1 extraction and existing-memory comparison boundary, with the
/// model call routed through Hide's logged-in provider boundary.
pub const MEM0_OSS_PIN: &str = "mem0ai/mem0@v2.1.0";
pub const MEM0_OSS_COMMIT: &str = "19f713408273fb1d657daa38d7b82ccf496d36d5";
pub const MEM0_PROMPTS_SHA256: &str =
    "10bc8a34b3b5f0ce24560a2a3190c9112b979a891b981f48393bbd168d915a5c";
pub const MEM0_PIPELINE_SHA256: &str =
    "5b1b75e2f00aca7bd368a6e9cd5905145d60fd05a0e36d6b1ef3e2f1b4f28ca1";
pub const MEM0_SCORING_SHA256: &str =
    "9a4313fda723ad05cb52278e9ef0b9b5792b71fb3b41ba6318410121022e4527";
pub const MEM0_UPSTREAM_MANIFEST: &str = include_str!("../mem0-upstream.json");
pub const SCHEMA_VERSION: &str = "mem0-v2.1-project-memory-v1";

const SYSTEM: &str = r#"You are the Mem0 memory engine inside a coding workspace.
Extract only durable project facts, accepted decisions, reusable rules, and repeat-prevention lessons grounded in the supplied human and assistant events.
Discard proposals, transient progress, isolated error strings, guesses, and facts directly readable from current source code.
Compare each candidate with the supplied active memories and return exactly one relation: new, same, supersedes, conflicts, or discard.
Use supersedes only when a direct human event explicitly corrects the same subject. Use conflicts when authority is unclear.
Keep every candidate independently understandable and independently removable. Preserve the language of its source.
Never output credentials or secret candidates. Return only JSON matching the schema."#;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Mem0OutputError {
    InputTooLarge { measured: usize },
    InvalidShape(String),
    SecretCandidate,
}

impl Display for Mem0OutputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InputTooLarge { measured } => {
                write!(formatter, "memory_input_over_cap:{measured}")
            }
            Self::InvalidShape(reason) => write!(formatter, "memory_output_invalid:{reason}"),
            Self::SecretCandidate => formatter.write_str("memory_output_secret_candidate"),
        }
    }
}

impl std::error::Error for Mem0OutputError {}

#[derive(Default)]
pub struct Mem0Adapter;

impl Mem0Adapter {
    pub fn request(
        &self,
        request_id: impl Into<String>,
        project_id: &str,
        session_id: &str,
        normalized_events_json: &str,
        active_memories_json: &str,
    ) -> Result<AiRequest, Mem0OutputError> {
        let events = redact(normalized_events_json);
        let input = json!({
            "project_id": project_id,
            "session_id": session_id,
            "new_events": events.text,
            "active_memories": active_memories_json,
            "engine_pin": MEM0_OSS_PIN,
            "engine_commit": MEM0_OSS_COMMIT,
        })
        .to_string();
        if input.len() > ANALYSIS_INPUT_LIMIT_BYTES {
            return Err(Mem0OutputError::InputTooLarge {
                measured: input.len(),
            });
        }
        Ok(AiRequest {
            feature_id: "project_memory",
            request_id: RequestId(request_id.into()),
            subject_id: format!("{project_id}:{session_id}"),
            system: SYSTEM.to_owned(),
            input,
            output_schema: output_schema(),
            deadline: Duration::from_secs(45),
            schema_version: SCHEMA_VERSION,
        })
    }

    pub fn parse(&self, value: Value) -> Result<Vec<Candidate>, Mem0OutputError> {
        let output: Output = serde_json::from_value(value)
            .map_err(|error| Mem0OutputError::InvalidShape(error.to_string()))?;
        output
            .candidates
            .into_iter()
            .map(|candidate| {
                let redacted = redact(candidate.text.trim());
                if redacted.contains_secret_candidate {
                    return Err(Mem0OutputError::SecretCandidate);
                }
                if redacted.text.is_empty() || candidate.source_offsets.is_empty() {
                    return Err(Mem0OutputError::InvalidShape(
                        "empty candidate or provenance".to_owned(),
                    ));
                }
                let relation = match candidate.relation.as_str() {
                    "new" => CandidateRelation::New,
                    "same" => CandidateRelation::Same {
                        target_id: required_target(&candidate)?,
                    },
                    "supersedes" => CandidateRelation::Supersedes {
                        target_id: required_target(&candidate)?,
                    },
                    "conflicts" => CandidateRelation::Conflicts {
                        target_id: required_target(&candidate)?,
                    },
                    "discard" => CandidateRelation::Discard,
                    other => {
                        return Err(Mem0OutputError::InvalidShape(format!(
                            "unknown relation {other}"
                        )));
                    }
                };
                let kind = match candidate.kind.as_str() {
                    "fact" => CandidateKind::Fact,
                    "decision" => CandidateKind::Decision,
                    "rule" => CandidateKind::Rule,
                    "lesson" => CandidateKind::Lesson,
                    other => {
                        return Err(Mem0OutputError::InvalidShape(format!(
                            "unknown kind {other}"
                        )));
                    }
                };
                Ok(Candidate {
                    text: redacted.text,
                    kind,
                    confidence: candidate.confidence.clamp(0.0, 1.0),
                    salience: candidate.salience.clamp(0.0, 1.0),
                    source_offsets: candidate.source_offsets,
                    direct_human_source: candidate.direct_human_source,
                    relation,
                })
            })
            .collect()
    }
}

#[derive(Deserialize)]
struct Output {
    candidates: Vec<OutputCandidate>,
}

#[derive(Deserialize)]
struct OutputCandidate {
    text: String,
    kind: String,
    confidence: f64,
    #[serde(default = "default_salience")]
    salience: f64,
    source_offsets: Vec<u64>,
    direct_human_source: bool,
    relation: String,
    target_id: Option<String>,
}

fn default_salience() -> f64 {
    0.5
}

fn required_target(candidate: &OutputCandidate) -> Result<String, Mem0OutputError> {
    candidate
        .target_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| {
            Mem0OutputError::InvalidShape(format!("{} relation has no target", candidate.relation))
        })
}

fn output_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["candidates"],
        "properties": {
            "candidates": {
                "type": "array",
                "maxItems": 64,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["text", "kind", "confidence", "salience", "source_offsets", "direct_human_source", "relation", "target_id"],
                    "properties": {
                        "text": {"type": "string", "minLength": 1, "maxLength": 4000},
                        "kind": {"enum": ["fact", "decision", "rule", "lesson"]},
                        "confidence": {"type": "number", "minimum": 0, "maximum": 1},
                        "salience": {"type": "number", "minimum": 0, "maximum": 1},
                        "source_offsets": {"type": "array", "minItems": 1, "maxItems": 64, "items": {"type": "integer", "minimum": 0}},
                        "direct_human_source": {"type": "boolean"},
                        "relation": {"enum": ["new", "same", "supersedes", "conflicts", "discard"]},
                        "target_id": {"type": ["string", "null"]}
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_uses_the_pinned_engine_and_hide_provider_boundary() {
        let request = Mem0Adapter
            .request("r1", "project:1", "s1", "[]", "[]")
            .unwrap();
        assert_eq!(request.feature_id, "project_memory");
        assert_eq!(request.schema_version, SCHEMA_VERSION);
        assert!(request.input.contains(MEM0_OSS_PIN));
        assert!(request.input.contains(MEM0_OSS_COMMIT));
        assert_eq!(request.subject_id, "project:1:s1");

        let manifest: Value = serde_json::from_str(MEM0_UPSTREAM_MANIFEST).unwrap();
        assert_eq!(manifest["commit"], MEM0_OSS_COMMIT);
        assert_eq!(
            manifest["audited_files"]["mem0/configs/prompts.py"],
            MEM0_PROMPTS_SHA256
        );
        assert_eq!(
            manifest["audited_files"]["mem0/memory/main.py"],
            MEM0_PIPELINE_SHA256
        );
        assert_eq!(
            manifest["audited_files"]["mem0/utils/scoring.py"],
            MEM0_SCORING_SHA256
        );
    }

    #[test]
    fn invalid_relations_and_secret_candidates_are_rejected_before_write_authority() {
        let secret = json!({"candidates":[{
            "text":"api_key=super-secret-value-123", "kind":"fact", "confidence":1,
            "salience":1, "source_offsets":[1], "direct_human_source":true,
            "relation":"new", "target_id":null
        }]});
        assert_eq!(
            Mem0Adapter.parse(secret),
            Err(Mem0OutputError::SecretCandidate)
        );
        let unknown = json!({"candidates":[{
            "text":"Keep tests deterministic", "kind":"rule", "confidence":1,
            "salience":1, "source_offsets":[1], "direct_human_source":true,
            "relation":"rewrite", "target_id":null
        }]});
        assert!(matches!(
            Mem0Adapter.parse(unknown),
            Err(Mem0OutputError::InvalidShape(_))
        ));
    }

    #[test]
    fn korean_and_english_output_fixtures_preserve_language_and_relation_classes() {
        let fixture = json!({"candidates":[
            {
                "text":"워크트리마다 같은 Project 기억을 사용한다.", "kind":"decision",
                "confidence":0.95, "salience":0.8, "source_offsets":[4],
                "direct_human_source":true, "relation":"same", "target_id":"memory:korean"
            },
            {
                "text":"Keep hook retrieval read-only.", "kind":"rule",
                "confidence":0.9, "salience":0.7, "source_offsets":[8],
                "direct_human_source":false, "relation":"conflicts", "target_id":"memory:english"
            }
        ]});

        let candidates = Mem0Adapter.parse(fixture).unwrap();
        assert_eq!(
            candidates[0].text,
            "워크트리마다 같은 Project 기억을 사용한다."
        );
        assert_eq!(
            candidates[0].relation,
            CandidateRelation::Same {
                target_id: "memory:korean".to_owned()
            }
        );
        assert_eq!(candidates[1].text, "Keep hook retrieval read-only.");
        assert_eq!(
            candidates[1].relation,
            CandidateRelation::Conflicts {
                target_id: "memory:english".to_owned()
            }
        );
    }
}
