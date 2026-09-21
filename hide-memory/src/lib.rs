//! Project Memory domain boundary.
//!
//! Hide owns extraction, relation planning, durable SQLite rows, provenance,
//! lifecycle, query-dependent retrieval, token budgets, and the read-only hook
//! projection. Pinned upstream prompt assets are design references only; no
//! Mem0 package or service executes at runtime.

mod engine;
mod redaction;
mod store;

pub use engine::{
    DESIGN_REFERENCE_ADDITIVE_PROMPT_SHA256, DESIGN_REFERENCE_COMMIT, DESIGN_REFERENCE_MANIFEST,
    DESIGN_REFERENCE_PIN, DESIGN_REFERENCE_UPDATE_PROMPT_SHA256, HideNativeAnalyzer,
    HideNativeOutputError,
};
pub use redaction::{RedactedText, redact};
pub use store::{
    AnalysisBatch, ApplySummary, Candidate, CandidateKind, CandidateRelation, ConflictChoice,
    DeleteProjectOutcome, Injection, InjectionOutcome, MemoryDetail, MemoryError, MemoryItem,
    MemoryLifecycle, MemorySource, MemoryStore, ProjectMemoryState, RetrievalQuery,
    SessionCursorRecord, SessionSourceRecord, StoreMode,
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
/// Budget available after the helper reaches Rust code.
///
/// The public hook contract measures from process launch, so production keeps
/// startup, scheduling, stdout flush, and teardown inside the remaining 25 ms.
pub const HOOK_PROCESS_BUDGET_MS: u64 = 75;
