//! Session work records. Values cross worker boundaries; SQLite remains
//! exclusively owned by `hide-links`.

use crate::labels::analysis::LabelEnd;
use serde::{Deserialize, Serialize};

pub const REQUEST_LIMIT: usize = 2_000;
pub const TASK_LIMIT: usize = 200;
pub const MISSED_LIMIT: usize = 200;
pub const NEW_MISSED_LIMIT: usize = 3;
pub const MISSED_CHARS: usize = 80;
pub const CONTINUED_TASK_LIMIT: usize = 2;
pub const LEDGER_QUEUE_LIMIT: usize = 512;
pub const SEARCH_LIMIT: usize = 50;
pub const LEDGER_RPC_LIMIT: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissedSource {
    pub session: SessionIdentity,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueAttempt {
    pub repository: String,
    pub title: String,
    pub body: String,
    pub started_at_unix_ms: u64,
    pub reconcile: bool,
    pub number: Option<u64>,
    pub url: Option<String>,
}

/// Short store operations asked by a worker, never waited on by Runtime.
#[derive(Clone, Debug)]
pub enum LedgerCall {
    Record(SessionIdentity),
    ReserveIssue {
        source: MissedSource,
        repository: String,
        title: String,
        body: String,
        at_unix_ms: u64,
    },
    StartIssue {
        source: MissedSource,
        title: String,
        body: String,
        at_unix_ms: u64,
    },
    FinishIssue {
        source: MissedSource,
        number: u64,
        url: String,
    },
    UnknownIssue(MissedSource),
}

#[derive(Debug)]
pub enum LedgerAnswer {
    Record(Box<SessionRecord>),
    Attempt(IssueAttempt),
    Saved,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionIdentity {
    pub device_id: String,
    pub agent: String,
    pub session_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkAuthor {
    Operator,
    Parent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedRequest {
    pub offset: u64,
    pub at_unix_ms: u64,
    pub text: String,
    pub author: WorkAuthor,
    pub task_ids: Vec<String>,
    pub chapter_id: Option<String>,
    pub judged: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkChapter {
    pub id: String,
    pub goal: String,
    pub at_unix_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkTask {
    pub id: String,
    pub chapter_id: String,
    pub name: String,
    pub at_unix_ms: u64,
    pub last_at_unix_ms: u64,
    pub end: LabelEnd,
    pub done: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedKind {
    Leftover,
    Problem,
    FollowUp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedStatus {
    Open,
    Resolved,
    Issue,
    Dismissed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissedWork {
    pub id: String,
    pub kind: MissedKind,
    pub text: String,
    pub status: MissedStatus,
    pub at_unix_ms: u64,
    pub turn_offset: u64,
    pub issue_repository: Option<String>,
    pub issue_number: Option<u64>,
    pub creating: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewMissedWork {
    pub kind: MissedKind,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedMissedWork {
    pub id: String,
    pub issue_number: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskDecision {
    New {
        name: String,
    },
    Continue {
        ids: Vec<String>,
    },
    #[default]
    None,
}

/// The extra judgment travels with the existing turn-end answer, never in
/// an additional provider call.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnJudgment {
    pub task: TaskDecision,
    pub missed: Vec<NewMissedWork>,
    pub resolved: Vec<ResolvedMissedWork>,
    pub output_task_id: Option<String>,
}

#[derive(Clone, Debug)]
pub enum LedgerUpdate {
    /// A turn observed with summaries off, or whose END judgment failed.
    /// It remains a raw request even if the label itself is retried later.
    Unjudged {
        session: SessionIdentity,
        offsets: Vec<u64>,
    },
    ExcludeRequests {
        session: SessionIdentity,
        offsets: Vec<u64>,
    },
    Title {
        session: SessionIdentity,
        native_title: Option<String>,
        manual_title: Option<String>,
        state: String,
    },
    Requests {
        session: SessionIdentity,
        cwd: String,
        path: Option<String>,
        requests: Vec<RecordedRequest>,
    },
    Chapter {
        session: SessionIdentity,
        goal: String,
        at_unix_ms: u64,
    },
    TurnEnd {
        session: SessionIdentity,
        offset: u64,
        at_unix_ms: u64,
        end: LabelEnd,
        work_author: bool,
        judgment: TurnJudgment,
    },
    MissedStatus {
        session: SessionIdentity,
        id: String,
        status: MissedStatus,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LedgerContext {
    pub open_tasks: Vec<WorkTask>,
    pub open_missed: Vec<MissedWork>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorkOutput {
    pub kind: String,
    pub repository: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub stale: bool,
    pub last_read_at_unix_ms: Option<u64>,
    pub created_at_unix_ms: u64,
    pub task_id: Option<String>,
    pub source: SessionIdentity,
    pub source_name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SessionRecord {
    pub session: SessionIdentity,
    pub title: String,
    pub native_title: Option<String>,
    pub manual_title: Option<String>,
    pub state: Option<String>,
    pub last_end: Option<LabelEnd>,
    pub cwd: Option<String>,
    pub path: Option<String>,
    pub started_at_unix_ms: Option<u64>,
    pub ended_at_unix_ms: Option<u64>,
    pub chapters: Vec<WorkChapter>,
    pub tasks: Vec<WorkTask>,
    pub requests: Vec<RecordedRequest>,
    pub missed: Vec<MissedWork>,
    pub outputs: Vec<WorkOutput>,
    pub children: Vec<SessionRecord>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SessionRecordSnapshot {
    pub session: Option<SessionIdentity>,
    pub loading: bool,
    pub failure: Option<String>,
    pub record: Option<SessionRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SessionWorkSummary {
    pub session: SessionIdentity,
    pub title: String,
    pub native_title: Option<String>,
    pub manual_title: Option<String>,
    pub cwd: Option<String>,
    pub ended_at_unix_ms: Option<u64>,
    pub open_missed: usize,
    pub subtree_open_missed: usize,
    pub missed: Vec<MissedWork>,
}

impl LedgerUpdate {
    pub fn session(&self) -> &SessionIdentity {
        match self {
            Self::Unjudged { session, .. }
            | Self::ExcludeRequests { session, .. }
            | Self::Title { session, .. }
            | Self::Requests { session, .. }
            | Self::Chapter { session, .. }
            | Self::TurnEnd { session, .. }
            | Self::MissedStatus { session, .. } => session,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SessionSearchHit {
    pub session: SessionIdentity,
    pub title: String,
    pub matched: String,
    pub at_unix_ms: u64,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OutputCreator {
    pub source: SessionIdentity,
    pub kind: String,
    pub repository: String,
    pub number: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SessionSearchSnapshot {
    pub query: String,
    pub loading: bool,
    pub failure: Option<String>,
    pub results: Vec<SessionSearchHit>,
}
