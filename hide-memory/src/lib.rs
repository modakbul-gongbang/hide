//! Project Memory domain boundary.
//!
//! Hide owns extraction, relation planning, durable SQLite rows, provenance,
//! lifecycle, query-dependent retrieval, token budgets, and the read-only hook
//! projection.

mod engine;
mod evidence;
mod matcher;
mod redaction;
mod store;

pub use engine::{
    AliasBackfill, AnalysisOutput, HideNativeAnalyzer, HideNativeOutputError,
    SYSTEM_PROMPT_LIMIT_BYTES,
};
pub use evidence::{
    EVIDENCE_ROW_LIMIT, Evidence, EvidenceFact, EvidenceKind, NAME_CANDIDATE_LIMIT, WrittenNames,
    still_holds, written_names,
};
pub use redaction::{RedactedText, redact};
pub use store::{
    AnalysisBatch, ApplyOptions, ApplySummary, Candidate, CandidateKind, CandidateOutcome,
    CandidateRelation, CardEvidence, ComparisonCard, CursorReader, DeleteProjectOutcome,
    InjectedItem, Injection, InjectionKind, InjectionOutcome, MemoryDetail, MemoryError,
    MemoryItem, MemoryLifecycle, MemorySource, MemoryStore, PendingCard, PreviousRevision,
    ProjectMemoryState, Promotion, PromotionState, PromotionSuggestion, ReceiptRecord,
    RetrievalQuery, SessionCursorRecord, SessionSourceRecord, StoreMode, card_name,
    parse_card_name, repository_path,
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
/// The bar a rule or lesson card crosses to be suggested for a repository
/// file (PRD memory-card-quality D-09, D-10): the operator's words in this
/// many different sessions, or reads from this many different sessions
/// within the window, with its evidence unchanged since it was last verified.
pub const PROMOTION_HUMAN_SESSIONS: u64 = 2;
pub const PROMOTION_PULLED_SESSIONS: u64 = 3;
pub const PROMOTION_WINDOW_MS: u64 = 14 * 86_400_000;
/// The most suggestions open at once (B29).
pub const PROMOTION_OPEN_LIMIT: usize = 3;
/// New cards one analyzed session may make (PRD memory-card-quality D-02);
/// a card that is the same, a correction or a conflict does not count.
pub const NEW_CARDS_PER_SESSION: usize = 2;
/// A session with fewer of the operator's own events than this makes no new
/// card (D-03).
pub const OPERATOR_EVENTS_MINIMUM: u64 = 3;
/// The most characters of the reason an agent gives when it calls a card
/// outdated.
pub const OUTDATED_REASON_LIMIT_CHARS: usize = 500;
/// A card is verified again at most once in this long (D-07, B10).
pub const REVERIFY_INTERVAL_MS: u64 = 86_400_000;
/// Cards one pass verifies again, and cards one pass checks before a
/// suggestion (D-11): each is one provider call under the daily cap.
pub const REVERIFY_PER_PASS: usize = 8;
pub const PROMOTION_CHECKS_PER_PASS: usize = 3;
/// Budget available after the helper reaches Rust code.
///
/// The public hook contract measures from process launch, so production keeps
/// startup, scheduling, stdout flush, and teardown inside the remaining 25 ms.
pub const HOOK_PROCESS_BUDGET_MS: u64 = 75;
