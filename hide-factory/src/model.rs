//! The Factory's values: what a Factory, a Task, its card and its records are.
//!
//! Nothing here performs I/O. The engine changes these values, the store
//! saves them, and the CLI and the stage 2 screens read them.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Minutes and hours as the engine counts them: milliseconds since the epoch.
pub type UnixMs = u64;

pub const MINUTE_MS: u64 = 60_000;
pub const HOUR_MS: u64 = 60 * MINUTE_MS;
pub const DAY_MS: u64 = 24 * HOUR_MS;

/// One Factory per project. Its id is stable for the project's path, so a
/// closed Factory created again for the same project returns with its records
/// (B74).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Factory {
    pub id: String,
    /// Canonical path of the project's primary checkout.
    pub project: String,
    /// A short name the inbox shows next to each item.
    pub project_name: String,
    pub source: SourceKind,
    /// `owner/name` of a GitHub Factory.
    #[serde(default)]
    pub repo: Option<String>,
    /// The branch Tasks merge into, read at init.
    #[serde(default = "default_branch")]
    pub default_branch: String,
    pub config: Config,
    pub closed: bool,
    pub created_at: UnixMs,
    /// Next `T-<n>` number. Never lowered.
    pub next_task: u32,
    /// Next `L-<n>` number for a local project without its own local issue
    /// store. Never lowered.
    pub next_local_issue: u32,
    /// Auto merge stopped because main verification failed (B44, B47, B48).
    pub main: MainHealth,
    /// When outside work was last read, and how many reads failed in a row
    /// (B65 stale mark after three).
    pub outside_read_at: Option<UnixMs>,
    pub outside_read_failures: u32,
    /// Watch judgments sent today and the day they count for (B69).
    pub watch_day: u64,
    pub watch_sent_today: u32,
    pub watch_last_at: Option<UnixMs>,
}

fn default_branch() -> String {
    "main".into()
}

/// Where Tasks come from and where they merge (D-04, B3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// A project with a GitHub remote: issues, labels, pull requests.
    Github,
    /// A project without one: local issues `L-<n>` and a local merge to main.
    Local,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MainHealth {
    pub broken: bool,
    /// The last main commit whose verification passed.
    pub last_green: Option<String>,
    /// Why auto merge stays stopped; empty when it runs.
    pub reason: Option<String>,
    /// A recovery that could not decide or failed waits for a person (B48).
    pub needs_person: bool,
    /// Merges Factory made since the last green, oldest first.
    pub merges_since_green: Vec<LandedMerge>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandedMerge {
    pub task: String,
    pub sha: String,
    pub at: UnixMs,
}

/// The one verification a Factory chose (D-53).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Verification {
    /// Required checks of the pull request (GitHub only). Empty `checks`
    /// means the branch protection's required checks.
    Ci { checks: Vec<String> },
    /// Commands run in the Task worktree; every one must exit 0.
    Commands { commands: Vec<String> },
    /// No verification: auto merge is not available (B2).
    None,
}

impl Verification {
    pub fn exists(&self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeMode {
    Auto,
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    Claude,
    Codex,
}

impl Runtime {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }
}

/// When a natural-language check runs (D-15).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckPoint {
    Intake,
    AfterDone,
    Periodic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserCheck {
    pub at: CheckPoint,
    pub instruction: String,
}

/// The typed recovery actions (D-54). Nothing outside this list runs without
/// a person's approval.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    RemoveFinishedWorktrees,
    RestartWorker,
    SleepWakeWorker,
    SwitchRuntime,
    RetryReadsAndReconnect,
}

impl RecoveryAction {
    pub const ALL: [Self; 5] = [
        Self::RemoveFinishedWorktrees,
        Self::RestartWorker,
        Self::SleepWakeWorker,
        Self::SwitchRuntime,
        Self::RetryReadsAndReconnect,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::RemoveFinishedWorktrees => "remove_finished_worktrees",
            Self::RestartWorker => "restart_worker",
            Self::SleepWakeWorker => "sleep_wake_worker",
            Self::SwitchRuntime => "switch_runtime",
            Self::RetryReadsAndReconnect => "retry_reads_and_reconnect",
        }
    }
}

/// Every setting a Factory has, with the D-30 defaults (B66).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub verification: Verification,
    pub merge_mode: MergeMode,
    pub merge_method: MergeMethod,
    /// Optional quick check before a merge (D-46), run on the merged tree.
    pub quick_check: Option<String>,
    /// Concurrent workers on this machine across every Factory (D-30, D-45).
    pub max_workers: u32,
    pub question_deadline_ms: u64,
    pub stall_ms: u64,
    pub no_report_ms: u64,
    pub watch_interval_ms: u64,
    pub watch_daily_limit: u32,
    pub outside_read_ms: u64,
    pub cancel_keep_ms: u64,
    pub done_fold_ms: u64,
    pub archive_fold_ms: u64,
    pub new_task_limit: u32,
    pub verify_failure_limit: u32,
    pub verify_timeout_ms: u64,
    pub disk_floor_bytes: u64,
    pub default_runtime: Runtime,
    pub harness: Option<Harness>,
    pub autonomy: Vec<AutonomyScope>,
    pub autonomy_diff_limit: u32,
    pub recovery: Vec<RecoveryAction>,
    pub risk_paths: Vec<String>,
    pub checks: Vec<UserCheck>,
    pub prd_in_issue: bool,
    pub macos_notifications: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            verification: Verification::None,
            merge_mode: MergeMode::Manual,
            merge_method: MergeMethod::Merge,
            quick_check: None,
            max_workers: 5,
            question_deadline_ms: 24 * HOUR_MS,
            stall_ms: 30 * MINUTE_MS,
            no_report_ms: 2 * MINUTE_MS,
            watch_interval_ms: 30 * MINUTE_MS,
            watch_daily_limit: 5,
            outside_read_ms: 2 * MINUTE_MS,
            cancel_keep_ms: 7 * DAY_MS,
            done_fold_ms: 3 * DAY_MS,
            archive_fold_ms: 90 * DAY_MS,
            new_task_limit: 3,
            verify_failure_limit: 3,
            verify_timeout_ms: 60 * MINUTE_MS,
            disk_floor_bytes: 20 * 1024 * 1024 * 1024,
            default_runtime: Runtime::Claude,
            harness: None,
            autonomy: AutonomyScope::presets(),
            autonomy_diff_limit: 200,
            recovery: Vec::new(),
            risk_paths: Vec::new(),
            checks: Vec::new(),
            prd_in_issue: false,
            macos_notifications: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Harness {
    pub name: String,
    /// The natural-language "how to work" preset placed in the worker prompt.
    pub instructions: String,
}

/// A kind of Task a worker may create and start without a person (D-18).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutonomyScope {
    pub id: String,
    pub description: String,
    pub enabled: bool,
}

impl AutonomyScope {
    /// The presets hide offers; every one starts off (D-18, B32).
    pub fn presets() -> Vec<Self> {
        [
            ("flaky_test", "Fix a flaky test"),
            ("dependency_patch", "Update a dependency by a patch version"),
            ("lint_format", "Lint and format fixes"),
            ("docs_links", "Fix documentation links and typos"),
        ]
        .into_iter()
        .map(|(id, description)| Self {
            id: id.to_owned(),
            description: description.to_owned(),
            enabled: false,
        })
        .collect()
    }
}

/// The Task lifecycle (D-28, Q29) plus the two places the PRD names outside
/// the main line: paused (B54) and merged-awaiting-main (B43).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// 정리 중: the card has open questions or waits for its review.
    Drafting,
    /// 대기: Ready; waits for its predecessors, a slot or the environment.
    Waiting,
    /// 실행 중: a worker holds a slot.
    Running,
    /// 일시정지: a person paused it; slot released, worker asleep.
    Paused,
    /// 막힘: waits for an answer or a predecessor; slot released.
    Blocked,
    /// 검증 중: the worker reported done; Factory verifies.
    Verifying,
    /// 머지 대기: a person merges, requests changes or cancels.
    MergeWaiting,
    /// 머지됨: merged; main verification has not passed yet.
    Landed,
    /// 완료.
    Done,
    /// 멈춤: stopped without report, stalled, or verification failed three times.
    Stopped,
    /// 다시 올리기: reverted from main; runs again on the latest main.
    Relanding,
    /// 밖에서 진행 중: an outside pull request closes this Task's issue.
    Outside,
    /// 취소: kept seven days for revival.
    Cancelled,
}

impl TaskState {
    pub const ALL: [Self; 13] = [
        Self::Drafting,
        Self::Waiting,
        Self::Running,
        Self::Paused,
        Self::Blocked,
        Self::Verifying,
        Self::MergeWaiting,
        Self::Landed,
        Self::Done,
        Self::Stopped,
        Self::Relanding,
        Self::Outside,
        Self::Cancelled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Drafting => "drafting",
            Self::Waiting => "waiting",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Blocked => "blocked",
            Self::Verifying => "verifying",
            Self::MergeWaiting => "merge_waiting",
            Self::Landed => "landed",
            Self::Done => "done",
            Self::Stopped => "stopped",
            Self::Relanding => "relanding",
            Self::Outside => "outside",
            Self::Cancelled => "cancelled",
        }
    }

    /// Korean label the operator reads.
    pub fn label(self) -> &'static str {
        match self {
            Self::Drafting => "정리 중",
            Self::Waiting => "대기",
            Self::Running => "실행 중",
            Self::Paused => "일시정지",
            Self::Blocked => "막힘",
            Self::Verifying => "검증 중",
            Self::MergeWaiting => "머지 대기",
            Self::Landed => "머지됨",
            Self::Done => "완료",
            Self::Stopped => "멈춤",
            Self::Relanding => "다시 올리기",
            Self::Outside => "밖에서 진행 중",
            Self::Cancelled => "취소",
        }
    }

    /// The board column (D-47). Cancelled sits outside the board.
    pub fn column(self) -> Option<Column> {
        match self {
            Self::Drafting => Some(Column::Drafting),
            Self::Waiting => Some(Column::Waiting),
            Self::Done => Some(Column::Done),
            Self::Cancelled => None,
            _ => Some(Column::Running),
        }
    }

    /// Whether the Task holds one of the machine's worker slots.
    pub fn holds_slot(self) -> bool {
        matches!(self, Self::Running | Self::Relanding)
    }

    /// A predecessor in this state lets its dependents start (D-29: merged).
    pub fn merged(self) -> bool {
        matches!(self, Self::Landed | Self::Done)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    Drafting,
    Waiting,
    Running,
    Done,
}

impl Column {
    pub const ALL: [Self; 4] = [Self::Drafting, Self::Waiting, Self::Running, Self::Done];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Drafting => "drafting",
            Self::Waiting => "waiting",
            Self::Running => "running",
            Self::Done => "done",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Drafting => "정리 중",
            Self::Waiting => "대기",
            Self::Running => "실행 중",
            Self::Done => "완료",
        }
    }
}

/// The card's fields (D-06). Goal, criteria, out-of-scope, open decisions and
/// dependencies come from the producer; the review only adds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub title: String,
    pub goal: String,
    pub criteria: Vec<String>,
    pub out_of_scope: Vec<String>,
    pub open_decisions: Vec<String>,
    /// Task ids or issue references this Task waits on.
    pub depends_on: Vec<String>,
    /// Waits on a Task of another repository: shown only (B64).
    pub external: Vec<String>,
}

/// What a person sets on a card (D-06); empty means the Factory default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanFields {
    pub review_directly: bool,
    pub priority: i32,
    pub merge_mode: Option<MergeMode>,
    pub runtime: Option<Runtime>,
}

/// The record a Task is shown by: its issue once Ready, else its Task id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IssueRef {
    Github { number: u64 },
    Local { number: u32 },
}

impl IssueRef {
    pub fn display(&self) -> String {
        match self {
            Self::Github { number } => format!("#{number}"),
            Self::Local { number } => format!("L-{number}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// Absolute path of the private copy the worker reads.
    pub path: String,
    pub sha256: String,
    pub version: u32,
    pub original: String,
}

/// Where a question came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionOrigin {
    Review,
    Worker,
    Check,
    Engine,
}

/// What answering a question does once chosen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuestionKind {
    /// An open decision while drafting (no deadline progress, D-09).
    Intake,
    /// The review proposed a split (D-55).
    Split { pieces: Vec<SplitPiece> },
    /// A worker question with a default action (B26).
    Default,
    /// A worker question that cannot proceed without an answer (B27).
    Blocking,
    /// A scope change: a person approves a wider scope (B29 ③).
    ScopeChange,
    /// The new-task cap was reached (B31).
    NewTaskCap,
    /// A worker proposed a new Task outside autonomy (B30).
    ProposedTask { draft: Box<Card> },
    /// A stop, a main break or a recovery that needs a person; the choices
    /// are the actions.
    Action,
    /// A label-path card needs a person's confirmation (D-48, B15).
    ConfirmCard,
    /// A recovery proposal with the exact command and impact (B61).
    Proposal { command: String, impact: String },
    /// Notification only (unrelated discovery, outside change, watch).
    Notice,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitPiece {
    pub title: String,
    pub goal: String,
    pub criteria: Vec<String>,
    /// Indexes of earlier pieces this piece waits on.
    pub after: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub origin: QuestionOrigin,
    pub kind: QuestionKind,
    pub text: String,
    pub suggestion: String,
    pub default_action: Option<String>,
    pub deadline: Option<UnixMs>,
    pub asked_at: UnixMs,
    pub choices: Vec<String>,
    pub answer: Option<Answer>,
    /// The letter a worker asked through, answered by reply.
    pub letter: Option<String>,
}

impl Question {
    pub fn open(&self) -> bool {
        self.answer.is_none()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub text: String,
    /// The suggestion or default chosen, when one was.
    pub chose: Option<String>,
    /// Who relayed it: a pane id for an operator pane, or `deadline`.
    pub relayed_by: String,
    pub at: UnixMs,
}

/// The five discovery classes (D-16, B29).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryClass {
    InScope = 1,
    Decision = 2,
    ScopeChange = 3,
    Prerequisite = 4,
    Unrelated = 5,
}

impl DiscoveryClass {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "in-scope" | "in_scope" => Some(Self::InScope),
            "decision" => Some(Self::Decision),
            "scope-change" | "scope_change" => Some(Self::ScopeChange),
            "prerequisite" => Some(Self::Prerequisite),
            "unrelated" => Some(Self::Unrelated),
            _ => None,
        }
    }

    /// How far the class is from a person: a reclassification may only move
    /// toward a person (B30). ① and ② are the agent's, ③ and ⑤ go to a
    /// person, ④ creates work a person approves.
    pub fn toward_person(self) -> u8 {
        match self {
            Self::InScope => 0,
            Self::Decision => 1,
            Self::Prerequisite => 2,
            Self::ScopeChange => 3,
            Self::Unrelated => 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovery {
    pub id: String,
    pub class: DiscoveryClass,
    pub text: String,
    pub at: UnixMs,
    /// The Task a prerequisite proposal created, once it exists.
    pub task: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub text: String,
    pub by: String,
    pub at: UnixMs,
}

/// A person gate that sends an auto Task to merge waiting (D-25, B39, B41).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    ReviewDirectly,
    ApprovedScopeChange,
    BreakingChange,
    NoVerification,
    RiskPath,
    ManualMode,
    OpenQuestion,
    CheckFailed,
    AutonomyDiff,
    DirtyMain,
}

impl Gate {
    pub fn reason(self) -> &'static str {
        match self {
            Self::ReviewDirectly => "직접 확인 표시",
            Self::ApprovedScopeChange => "승인된 범위 변경",
            Self::BreakingChange => "보고된 breaking change 또는 공개 계약 변경",
            Self::NoVerification => "검증 없음",
            Self::RiskPath => "위험 경로 변경",
            Self::ManualMode => "manual 머지",
            Self::OpenQuestion => "열린 질문",
            Self::CheckFailed => "점검을 하지 못함",
            Self::AutonomyDiff => "자율 처리 diff 상한 초과",
            Self::DirtyMain => "main checkout에 커밋 안 된 변경",
        }
    }
}

/// Why a Task stopped (D-28, B24, B37, B31).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    NoReport,
    Stalled,
    VerifyFailed,
    NewTaskCap,
    EnvironmentRepeated,
}

impl StopReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::NoReport => "보고 없이 멈춤",
            Self::Stalled => "정체",
            Self::VerifyFailed => "검증 3회 실패",
            Self::NewTaskCap => "새 Task 상한",
            Self::EnvironmentRepeated => "같은 환경 실패 반복",
        }
    }
}

/// The worker a Task runs, as the runtime adapter reported it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRef {
    pub name: String,
    pub pane: Option<String>,
    pub runtime: Runtime,
    pub worktree: String,
    pub branch: String,
    pub started_at: UnixMs,
    pub asleep: bool,
}

/// A verification attempt in progress or finished.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    /// n of the n/3 count: fixes the worker submitted (D-53).
    pub number: u32,
    pub commit: Option<String>,
    pub started_at: UnixMs,
    pub stage: AttemptStage,
    pub outcome: Option<AttemptOutcome>,
    pub log: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStage {
    /// After done, in the Task worktree or the pull request's checks.
    Task,
    /// Before merge, on the latest main merged in (verify factories, B38).
    PreMerge,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum AttemptOutcome {
    Passed,
    Failed { check: String, link: String },
    Environment { signal: String, check: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
    pub head: String,
    /// The Factory opened it (B36); otherwise a harness or an outside PR did.
    pub by_factory: bool,
    pub open: bool,
}

/// A Task: the Factory's unit of execution (D-05).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub factory: String,
    /// `T-<n>`.
    pub id: String,
    pub seq: u32,
    pub issue: Option<IssueRef>,
    pub card: Card,
    pub human: HumanFields,
    pub state: TaskState,
    pub state_since: UnixMs,
    pub created_at: UnixMs,
    pub updated_at: UnixMs,
    /// The Task that proposed this one; its worker may not propose (depth 1).
    pub proposed_by: Option<String>,
    /// Autonomy scope id when started without a person (B32).
    pub autonomy: Option<String>,
    pub attachments: Vec<Attachment>,
    pub questions: Vec<Question>,
    pub discoveries: Vec<Discovery>,
    pub decisions: Vec<DecisionRecord>,
    pub flags: Vec<String>,
    pub review: ReviewState,
    pub producer_pane: Option<String>,
    pub worker: Option<WorkerRef>,
    pub attempts: Vec<Attempt>,
    /// n of n/3: verification failures counted against the Task (D-53).
    pub failures: u32,
    /// Environment failures in a row, for the reclassification rule (B60).
    pub environment_failures: u32,
    pub pr: Option<PullRequest>,
    pub merge_sha: Option<String>,
    pub gates: Vec<Gate>,
    pub stop: Option<StopReason>,
    pub breaking: bool,
    pub scope_approved: bool,
    pub new_tasks: u32,
    pub new_task_cap_extended: bool,
    pub cancelled_at: Option<UnixMs>,
    /// State the Task was cancelled from, for revival (B55).
    pub cancelled_from: Option<TaskState>,
    pub purged: bool,
    pub done_at: Option<UnixMs>,
    /// A person has seen the completion (the done column's unread dot).
    pub seen: bool,
    /// Worker reported done and then went quiet; tracked for B24.
    pub last_report_at: Option<UnixMs>,
    pub idle_since: Option<UnixMs>,
    /// Intent keys of external writes already made (B73).
    pub writes: BTreeSet<String>,
    /// Environment hold: a start was refused by the pre-start check (B57).
    pub held: Option<String>,
    pub label_path: bool,
    pub source_body_hash: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    /// Not reviewed yet.
    #[default]
    Pending,
    /// The judgment is queued or running.
    Requested { at: UnixMs },
    /// The review answered.
    Done { result: ReviewResult },
    /// The review could not run (B19); a person is asked to fix the provider.
    Failed { reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewResult {
    Ready,
    NeedsAnswers,
    Split,
}

impl Task {
    /// A new Task in drafting, before its review (D-05).
    pub fn draft(factory: &str, id: &str, seq: u32, card: Card, now: UnixMs) -> Self {
        Self {
            factory: factory.to_owned(),
            id: id.to_owned(),
            seq,
            issue: None,
            card,
            human: HumanFields::default(),
            state: TaskState::Drafting,
            state_since: now,
            created_at: now,
            updated_at: now,
            proposed_by: None,
            autonomy: None,
            attachments: Vec::new(),
            questions: Vec::new(),
            discoveries: Vec::new(),
            decisions: Vec::new(),
            flags: Vec::new(),
            review: ReviewState::Pending,
            producer_pane: None,
            worker: None,
            attempts: Vec::new(),
            failures: 0,
            environment_failures: 0,
            pr: None,
            merge_sha: None,
            gates: Vec::new(),
            stop: None,
            breaking: false,
            scope_approved: false,
            new_tasks: 0,
            new_task_cap_extended: false,
            cancelled_at: None,
            cancelled_from: None,
            purged: false,
            done_at: None,
            seen: true,
            last_report_at: None,
            idle_since: None,
            writes: BTreeSet::new(),
            held: None,
            label_path: false,
            source_body_hash: None,
        }
    }

    pub fn display_id(&self) -> String {
        self.issue
            .as_ref()
            .map(IssueRef::display)
            .unwrap_or_else(|| self.id.clone())
    }

    pub fn open_questions(&self) -> impl Iterator<Item = &Question> {
        self.questions.iter().filter(|question| question.open())
    }

    pub fn merge_mode(&self, factory: &Factory) -> MergeMode {
        if !factory.config.verification.exists() {
            return MergeMode::Manual;
        }
        self.human.merge_mode.unwrap_or(factory.config.merge_mode)
    }

    pub fn runtime(&self, factory: &Factory) -> Runtime {
        self.human.runtime.unwrap_or(factory.config.default_runtime)
    }

    /// The card a person must look at: blocked, stopped, merge waiting, or
    /// an open question (D-28, D-47).
    pub fn needs_person(&self) -> bool {
        matches!(
            self.state,
            TaskState::Blocked | TaskState::Stopped | TaskState::MergeWaiting
        ) || self
            .open_questions()
            .any(|question| !matches!(question.kind, QuestionKind::Notice))
    }

    pub fn branch_slug(&self) -> String {
        let mut slug = String::new();
        for character in self.card.title.chars() {
            if character.is_ascii_alphanumeric() {
                slug.push(character.to_ascii_lowercase());
            } else if !slug.ends_with('-') && !slug.is_empty() {
                slug.push('-');
            }
            if slug.len() >= 32 {
                break;
            }
        }
        let slug = slug.trim_matches('-').to_owned();
        let number = match &self.issue {
            Some(IssueRef::Github { number }) => number.to_string(),
            Some(IssueRef::Local { number }) => format!("l{number}"),
            None => self.id.to_ascii_lowercase(),
        };
        if slug.is_empty() {
            format!("factory/{number}")
        } else {
            format!("factory/{number}-{slug}")
        }
    }
}
