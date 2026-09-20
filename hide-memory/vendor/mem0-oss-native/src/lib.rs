//! Audited native boundary for Mem0 OSS v2.1.0.
//!
//! Upstream Mem0 is Python. Hide cannot embed a Python runtime, start a Mem0
//! service, or require a second credential or vector database. This crate is
//! therefore the pinned runtime dependency that owns the upstream extraction,
//! relation-planning, and hybrid-search semantics that have native equivalents.
//! Hide's adapter remains authoritative for provider routing, Project identity,
//! persistence, provenance, lifecycle, privacy, and write transactions.

pub const OSS_PIN: &str = "mem0ai/mem0@v2.1.0";
pub const OSS_COMMIT: &str = "19f713408273fb1d657daa38d7b82ccf496d36d5";
pub const PROMPTS_SHA256: &str = "10bc8a34b3b5f0ce24560a2a3190c9112b979a891b981f48393bbd168d915a5c";
pub const PIPELINE_SHA256: &str =
    "5b1b75e2f00aca7bd368a6e9cd5905145d60fd05a0e36d6b1ef3e2f1b4f28ca1";
pub const SCORING_SHA256: &str = "9a4313fda723ad05cb52278e9ef0b9b5792b71fb3b41ba6318410121022e4527";
pub const ADDITIVE_PROMPT_SHA256: &str =
    "b9b3e71d9f73b8d9aefbfd6dfd3e6f1d425ce8cd100fbc969ba15e8ae013ad48";
pub const UPDATE_PROMPT_SHA256: &str =
    "18af574579716b35181914dcdeeed6840cea8c4b50342ebe378e1b3452668a4d";
pub const UPSTREAM_MANIFEST: &str = include_str!("../../../mem0-upstream.json");

const ADDITIVE_EXTRACTION_PROMPT: &str = include_str!("../../mem0/additive-extraction.prompt.txt");
const UPDATE_MEMORY_PROMPT: &str = include_str!("../../mem0/update-memory.prompt.txt");

pub const ENTITY_BOOST_WEIGHT: f64 = 0.5;
pub const DEFAULT_SEMANTIC_THRESHOLD: f64 = 0.1;

/// Build the exact Mem0 extraction and update-planning system prompt, followed
/// by the host policy that narrows the engine to Project Memory.
pub fn system_prompt(host_policy: &str) -> String {
    format!("{ADDITIVE_EXTRACTION_PROMPT}\n\n{UPDATE_MEMORY_PROMPT}\n\n{host_policy}")
}

/// Query-length-adaptive sigmoid parameters from Mem0 v2.1.0
/// `mem0/utils/scoring.py::get_bm25_params`.
pub fn bm25_params(term_count: usize) -> (f64, f64) {
    match term_count.max(1) {
        1..=3 => (5.0, 0.7),
        4..=6 => (7.0, 0.6),
        7..=9 => (9.0, 0.5),
        10..=15 => (10.0, 0.5),
        _ => (12.0, 0.5),
    }
}

/// Mem0 v2.1.0 logistic normalization for a raw BM25 score.
pub fn normalize_bm25(raw_score: f64, midpoint: f64, steepness: f64) -> f64 {
    1.0 / (1.0 + (-steepness * (raw_score - midpoint)).exp())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchSignals {
    pub semantic_score: f64,
    pub bm25_score: f64,
    /// Entity/path overlap before Mem0's 0.5 entity weight is applied.
    pub entity_overlap: f64,
}

/// Native equivalent of Mem0 v2.1.0 `score_and_rank` for one result.
///
/// `has_bm25` and `has_entity` describe whether those result sets exist for
/// the whole query, matching upstream's dynamic denominator. The semantic
/// threshold is applied before additive scoring.
pub fn hybrid_score(
    signals: SearchSignals,
    has_bm25: bool,
    has_entity: bool,
    threshold: f64,
) -> Option<f64> {
    let semantic = signals.semantic_score.clamp(0.0, 1.0);
    if semantic < threshold {
        return None;
    }
    let bm25 = if has_bm25 {
        signals.bm25_score.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let entity = if has_entity {
        signals.entity_overlap.clamp(0.0, 1.0) * ENTITY_BOOST_WEIGHT
    } else {
        0.0
    };
    let maximum =
        1.0 + if has_bm25 { 1.0 } else { 0.0 } + if has_entity { ENTITY_BOOST_WEIGHT } else { 0.0 };
    Some(((semantic + bm25 + entity) / maximum).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_hybrid_scoring_uses_dynamic_signal_denominator() {
        let signals = SearchSignals {
            semantic_score: 0.8,
            bm25_score: 0.5,
            entity_overlap: 0.4,
        };
        assert_eq!(hybrid_score(signals, true, true, 0.1), Some(0.6));
        assert_eq!(hybrid_score(signals, true, false, 0.1), Some(0.65));
        assert_eq!(hybrid_score(signals, false, true, 0.1), Some(2.0 / 3.0));
        assert_eq!(hybrid_score(signals, false, false, 0.1), Some(0.8));
        assert_eq!(hybrid_score(signals, true, true, 0.9), None);
    }

    #[test]
    fn upstream_assets_are_compile_time_dependencies() {
        assert!(ADDITIVE_EXTRACTION_PROMPT.contains("# ROLE"));
        assert!(UPDATE_MEMORY_PROMPT.contains("You can perform four operations"));
        assert!(UPSTREAM_MANIFEST.contains(OSS_COMMIT));
    }
}
