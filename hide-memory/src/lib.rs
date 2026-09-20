//! Project Memory domain boundary.
//!
//! Mem0 OSS supplies the extraction and relation-planning semantics through
//! [`Mem0Adapter`]. Hide remains authoritative for project identity, durable
//! SQLite rows, provenance, lifecycle, token budgets, and the read-only hook
//! projection.

mod mem0;
mod redaction;
mod store;

pub use mem0::{
    MEM0_ADDITIVE_PROMPT_SHA256, MEM0_OSS_COMMIT, MEM0_OSS_PIN, MEM0_PIPELINE_SHA256,
    MEM0_PROMPTS_SHA256, MEM0_SCORING_SHA256, MEM0_UPDATE_PROMPT_SHA256, MEM0_UPSTREAM_MANIFEST,
    Mem0Adapter, Mem0OutputError,
};
pub use redaction::{RedactedText, redact};
pub use store::{
    AnalysisBatch, ApplySummary, Candidate, CandidateKind, CandidateRelation, ConflictChoice,
    Injection, InjectionOutcome, MemoryDetail, MemoryError, MemoryItem, MemoryLifecycle,
    MemorySource, MemoryStore, ProjectMemoryState, RetrievalQuery, SessionCursorRecord,
    SessionSourceRecord, StoreMode,
};

pub const ACTIVE_MEMORY_LIMIT: usize = 10_000;
pub const MEMORY_BODY_LIMIT_CHARS: usize = 4_000;
pub const ANALYSIS_INPUT_LIMIT_BYTES: usize = 64 * 1024;
pub const HOOK_INPUT_LIMIT_BYTES: usize = 256 * 1024;
pub const HOOK_CANDIDATE_LIMIT: usize = 60;
pub const SESSION_START_ITEM_LIMIT: usize = 5;
pub const PROMPT_ITEM_LIMIT: usize = 3;
pub const INJECTION_TOKEN_LIMIT: usize = 600;
pub const HOOK_DEADLINE_MS: u64 = 100;
