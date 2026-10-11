//! Project Memory domain boundary.
//!
//! Hide owns extraction, relation planning, durable SQLite rows, provenance,
//! lifecycle, query-dependent retrieval, token budgets, and the read-only hook
//! projection.

mod engine;
mod matcher;
mod redaction;
mod store;

pub use engine::{
    AliasBackfill, AnalysisOutput, HideNativeAnalyzer, HideNativeOutputError,
    SYSTEM_PROMPT_LIMIT_BYTES,
};
pub use redaction::{RedactedText, redact};
pub use store::{
    AnalysisBatch, ApplySummary, Candidate, CandidateKind, CandidateOutcome, CandidateRelation,
    ComparisonCard, ConflictChoice, CursorReader, DeleteProjectOutcome, InjectedItem, Injection,
    InjectionKind, InjectionOutcome, MemoryDetail, MemoryError, MemoryItem, MemoryLifecycle,
    MemorySource, MemoryStore, ProjectMemoryState, ReceiptRecord, RetrievalQuery,
    SessionCursorRecord, SessionSourceRecord, StoreMode, card_name, parse_card_name,
};

/// The Memory store's file name inside a state folder.
pub const DATABASE_FILE: &str = "project-memory.sqlite3";

/// The one place the store's location is spelled: the core writes it and the
/// hooks read it, each from the state folder it resolves.
pub fn database_path(state_dir: &std::path::Path) -> std::path::PathBuf {
    state_dir.join(DATABASE_FILE)
}

pub const ACTIVE_MEMORY_LIMIT: usize = 10_000;
pub const MEMORY_BODY_LIMIT_CHARS: usize = 4_000;
pub const ANALYSIS_INPUT_LIMIT_BYTES: usize = 64 * 1024;
pub const HOOK_INPUT_LIMIT_BYTES: usize = 256 * 1024;
pub const HOOK_CANDIDATE_LIMIT: usize = 60;
/// Index lines a session start gets (D-04).
pub const INDEX_LINE_LIMIT: usize = 20;
/// Lines `hide memory search` answers with (B15).
pub const SEARCH_RESULT_LIMIT: usize = 10;
/// Cards one analysis compares new text against (D-13).
pub const COMPARISON_LIMIT: usize = 30;
pub const PROMPT_ITEM_LIMIT: usize = 3;
pub const INJECTION_TOKEN_LIMIT: usize = 600;
/// A card's index line, in characters (D-23: an old card's is its first 60).
pub const TITLE_LIMIT_CHARS: usize = 60;
pub const ALIAS_LIMIT: usize = 12;
pub const ALIAS_LIMIT_CHARS: usize = 40;
/// Files one card cites, each a repository-relative path (D-26).
pub const CITED_PATH_LIMIT: usize = 8;
pub const CITED_PATH_LIMIT_BYTES: usize = 240;
/// Receipt rows a Project keeps (B26); past it, receipts of sessions quiet
/// for [`RECEIPT_STALE_AFTER_MS`] go first, and a receipt that still does not
/// fit is not recorded.
pub const RECEIPT_LIMIT: usize = 20_000;
pub const RECEIPT_STALE_AFTER_MS: u64 = 30 * 86_400_000;
/// Analysis calls one machine makes in a UTC day (D-10 a).
pub const DAILY_ANALYSIS_CALL_LIMIT: u64 = 1_000;
pub const HOOK_DEADLINE_MS: u64 = 100;
/// Budget available after the helper reaches Rust code.
///
/// The public hook contract measures from process launch, so production keeps
/// startup, scheduling, stdout flush, and teardown inside the remaining 25 ms.
pub const HOOK_PROCESS_BUDGET_MS: u64 = 75;
