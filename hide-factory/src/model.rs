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
    /// The person's approval of this Factory's GitHub reads and writes; no
    /// GitHub write happens without it (D-62).
    #[serde(default)]
    pub github_approval: Option<GithubApproval>,
    /// The operator paused the whole Factory (D-48): no starts, no AI
    /// judgments, no auto merge, workers asleep.
    #[serde(default)]
    pub paused: bool,
    /// Observer calls sent on `observer_day` (local days since the epoch).
    #[serde(default)]
    pub observer_day: u64,
    #[serde(default)]
    pub observer_calls: u32,
    /// The local day the daily cap notice was last given (D-34: once a day).
    #[serde(default)]
    pub observer_cap_notice_day: u64,
}

/// Who approved a GitHub Factory's access, for which repository, and when,
/// recorded at `init --confirm` (D-62).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubApproval {
    /// The `gh` login the Factory acts as.
    pub account: String,
    pub repo: String,
    pub at: UnixMs,
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
    /// A recovery (finding or reverting the failing merge) is running.
    #[serde(default)]
    pub recovering: bool,
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
    /// Whether the Factory verifies at all.
    pub fn configured(&self) -> bool {
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

/// An agent a worker runs, by the Herdr kind its adapter declares (D-28).
/// Only an agent whose adapter declares a start is one; what it can do
/// beyond starting (waking, resuming, its texts) is read from the same
/// declaration. Stored as that kind, so `claude` and `codex` records an
/// earlier build wrote read unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Runtime(&'static str);

impl Runtime {
    pub const CLAUDE: Self = Self("claude");
    pub const CODEX: Self = Self("codex");

    pub fn as_str(self) -> &'static str {
        self.0
    }

    /// Any id or alias of an agent whose adapter declares a start.
    pub fn parse(value: &str) -> Option<Self> {
        hide_agent_adapter::start_kind(value).map(Self)
    }

    pub fn adapter(self) -> &'static hide_agent_adapter::AgentAdapter {
        hide_agent_adapter::adapter(self.0).expect("a Runtime is made only from a declared start")
    }

    /// The agent's display name.
    pub fn label(self) -> &'static str {
        self.adapter().label
    }

    /// Every agent a Factory can start, in the adapters' support order.
    pub fn all() -> impl Iterator<Item = Self> {
        hide_agent_adapter::START_KINDS
            .iter()
            .map(|kind| Self(kind))
    }

    /// How the agent's start takes a model and an effort.
    pub fn launch_options(self) -> Option<hide_agent_adapter::LaunchOptions> {
        self.adapter().start.map(|start| start.options())
    }
}

impl Serialize for Runtime {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for Runtime {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value)
            .ok_or_else(|| serde::de::Error::custom(format!("not a startable agent: {value}")))
    }
}

/// One worker a Factory may start: an agent, optionally its model and
/// effort, and the operator's one line about when it fits (D-41).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerCandidate {
    pub agent: Runtime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default)]
    pub description: String,
}

impl WorkerCandidate {
    pub fn bare(agent: Runtime) -> Self {
        Self {
            agent,
            model: None,
            effort: None,
            description: String::new(),
        }
    }

    /// The launch arguments its model and effort add, refused with the reason
    /// when the agent does not declare them.
    pub fn launch_arguments(&self) -> Result<Vec<String>, String> {
        if self.model.is_none() && self.effort.is_none() {
            return Ok(Vec::new());
        }
        let options = self
            .agent
            .launch_options()
            .ok_or_else(|| format!("{} takes no model or effort", self.agent.label()))?;
        options.arguments(self.model.as_deref(), self.effort.as_deref())
    }
}

/// The most worker candidates one Factory keeps (D-42).
pub const WORKER_CANDIDATE_LIMIT: usize = 5;
/// The Observer's daily call cap: default and range (D-17, D-34).
pub const OBSERVER_DAILY_DEFAULT: u32 = 100;
pub const OBSERVER_DAILY_RANGE: std::ops::RangeInclusive<u32> = 1..=1000;

/// Who decides a Factory's decision requests (D-14, D-18): on screen 직접,
/// 함께 and 맡김.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObserverMode {
    Manual,
    #[default]
    Assist,
    Autonomous,
}

impl ObserverMode {
    pub const ALL: [Self; 3] = [Self::Manual, Self::Assist, Self::Autonomous];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Assist => "assist",
            Self::Autonomous => "autonomous",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.as_str() == value)
    }
}

/// The agent, model and effort a Factory's AI judgments run on (D-40).
/// `None` in the config means the app's Hide AI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactoryAi {
    /// A Hide AI provider id (`claude`, `codex`, ...).
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
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
    /// The first candidate's agent. Read as the only candidate while
    /// `workers` is empty, which is how a Factory made before candidates
    /// reads until the operator next changes its workers (D-42).
    pub default_runtime: Runtime,
    /// Worker candidates, the first the default; empty means one candidate
    /// of `default_runtime` with the CLI's own model and effort.
    pub workers: Vec<WorkerCandidate>,
    pub observer_mode: ObserverMode,
    pub observer_daily_limit: u32,
    pub factory_ai: Option<FactoryAi>,
    pub harness: Option<Harness>,
    pub autonomy: Vec<AutonomyScope>,
    pub autonomy_diff_limit: u32,
    pub recovery: Vec<RecoveryAction>,
    pub risk_paths: Vec<String>,
    pub checks: Vec<UserCheck>,
    pub prd_in_issue: bool,
    pub macos_notifications: bool,
    /// Extra arguments each runtime's worker starts with, such as a
    /// permission mode the operator chose.
    #[serde(default)]
    pub worker_args: std::collections::BTreeMap<String, Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            verification: Verification::None,
            merge_mode: MergeMode::Manual,
            merge_method: MergeMethod::Merge,
            quick_check: None,
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
            default_runtime: Runtime::CLAUDE,
            workers: Vec::new(),
            observer_mode: ObserverMode::Assist,
            observer_daily_limit: OBSERVER_DAILY_DEFAULT,
            factory_ai: None,
            harness: None,
            autonomy: AutonomyScope::presets(),
            autonomy_diff_limit: 200,
            recovery: Vec::new(),
            risk_paths: Vec::new(),
            checks: Vec::new(),
            prd_in_issue: false,
            macos_notifications: false,
            worker_args: std::collections::BTreeMap::new(),
        }
    }
}

impl Config {
    /// The worker candidates in order, the first the default.
    pub fn candidates(&self) -> Vec<WorkerCandidate> {
        if self.workers.is_empty() {
            vec![WorkerCandidate::bare(self.default_runtime)]
        } else {
            self.workers.clone()
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
    Before,
    Moving,
    Stuck,
    Done,
}

impl Column {
    pub const ALL: [Self; 4] = [Self::Before, Self::Moving, Self::Stuck, Self::Done];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::Moving => "moving",
            Self::Stuck => "stuck",
            Self::Done => "done",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Before => "시작 전",
            Self::Moving => "진행 중",
            Self::Stuck => "멈춤",
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
    /// Intake's one-line description. Old records use the goal at read time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub criteria: Vec<String>,
    pub out_of_scope: Vec<String>,
    pub open_decisions: Vec<String>,
    /// Task ids or issue references this Task waits on.
    pub depends_on: Vec<String>,
    /// Waits on a Task of another repository: shown only (B64).
    pub external: Vec<String>,
}

impl Card {
    pub fn summary(&self) -> String {
        self.summary
            .as_deref()
            .filter(|summary| !summary.trim().is_empty())
            .map(short_summary)
            .unwrap_or_else(|| goal_summary(&self.goal, &self.title))
    }
}

/// A bounded, single-line description; count Unicode characters, not bytes.
pub fn short_summary(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= 60 {
        line
    } else {
        format!("{}…", line.chars().take(59).collect::<String>().trim_end())
    }
}

/// Legacy and failed-review cards need no migration or additional AI call.
pub fn goal_summary(goal: &str, title: &str) -> String {
    let body = goal
        .lines()
        .map(str::trim)
        .filter(|line| {
            let heading = line.trim_matches(['*', '_', ':', ' ']);
            !line.is_empty()
                && !line.starts_with(['#', '<'])
                && !matches!(*line, "---" | "***")
                && heading != title.trim()
                && ![
                    "goal",
                    "summary",
                    "description",
                    "task",
                    "목표",
                    "요약",
                    "설명",
                    "작업",
                ]
                .iter()
                .any(|name| heading.eq_ignore_ascii_case(name))
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut sentence = String::new();
    let mut chars = body.chars().peekable();
    while let Some(ch) = chars.next() {
        sentence.push(ch);
        if matches!(ch, '.' | '!' | '?') && chars.peek().is_none_or(|next| next.is_whitespace()) {
            break;
        }
    }
    short_summary(&sentence)
}

/// What a person sets on a card (D-06); empty means the Factory default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanFields {
    pub review_directly: bool,
    pub priority: i32,
    pub merge_mode: Option<MergeMode>,
    /// Pins the agent's first candidate (`add --runtime`).
    pub runtime: Option<Runtime>,
    /// Pins a candidate by its place in the list, 0 first (`add --worker`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<usize>,
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
pub struct CardChange {
    pub card: Card,
    pub attachment: Option<Attachment>,
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
    /// A scope change: a person approves a wider scope (B29 ③). A card or
    /// PRD added again while the Task runs carries it here and becomes the
    /// Task's on approval (B16).
    ScopeChange {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        change: Option<Box<CardChange>>,
    },
    /// The new-task cap was reached (B31).
    NewTaskCap,
    /// A worker proposed a new Task outside autonomy (B30).
    ProposedTask {
        draft: Box<Card>,
        /// The prerequisite discovery it answers; on approval the proposer
        /// waits on the new Task (D-16 ④).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        discovery: Option<String>,
    },
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
    /// Where a decision request was sent and why (D-14, D-18).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<Routing>,
    /// What an engine notice says, for the notice group (D-43).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<NoticeCode>,
    /// The question an Observer notice is about, which "다른 답" overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refers_to: Option<String>,
}

/// The five kinds the Observer sorts a decision request into (D-14).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionKind {
    /// The answer is already there.
    A,
    /// A technical choice.
    B,
    /// A product or taste choice.
    C,
    /// A permission: cost, sign-in, deletion, security, outside effect, out
    /// of scope, irreversible.
    D,
    /// The card is wrong.
    E,
}

impl DecisionKind {
    pub const ALL: [Self; 5] = [Self::A, Self::B, Self::C, Self::D, Self::E];

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "A" => Some(Self::A),
            "B" => Some(Self::B),
            "C" => Some(Self::C),
            "D" => Some(Self::D),
            "E" => Some(Self::E),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
        }
    }
}

/// Where a decision request went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteTo {
    /// The Observer is judging it; a person may still answer first.
    Pending,
    Person,
    /// The Observer answered or applied its proposal.
    Observer,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Routing {
    pub to: RouteTo,
    /// The Factory's mode when the request arrived; a later change does not
    /// move it (D-18).
    pub mode: ObserverMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<DecisionKind>,
    /// The Observer's one-line reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Why it went to a person without the Observer's verdict: `failed`,
    /// `daily_limit`, `paused`, `queue_full` (B10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    /// E in assist: the Observer's fix, offered as a choice (D-33).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<ObserverProposal>,
    /// A person replaced the Observer's answer.
    #[serde(default)]
    pub overridden: bool,
}

/// What the Observer proposes for a wrong card (D-33).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObserverProposal {
    CardFix { card: Box<Card> },
    NewTask { card: Box<Card>, prerequisite: bool },
}

/// The choice an assist-mode E request carries for the Observer's proposal.
pub const PROPOSAL_CHOICE: &str = "AI 제안 적용";

/// Engine notices the notice group shows (D-32, D-34, D-38).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeCode {
    AiAnswered,
    AiCardFixed,
    AiNewTask,
    AiRiskMerge,
    DailyLimit,
}

impl NoticeCode {
    pub const ALL: [Self; 5] = [
        Self::AiAnswered,
        Self::AiCardFixed,
        Self::AiNewTask,
        Self::AiRiskMerge,
        Self::DailyLimit,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AiAnswered => "ai_answered",
            Self::AiCardFixed => "ai_card_fixed",
            Self::AiNewTask => "ai_new_task",
            Self::AiRiskMerge => "ai_risk_merge",
            Self::DailyLimit => "daily_limit",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::AiAnswered => "AI가 답함",
            Self::AiCardFixed => "AI가 카드를 고침",
            Self::AiNewTask => "AI가 새 Task를 만듦",
            Self::AiRiskMerge => "AI가 위험 경로 머지를 승인함",
            Self::DailyLimit => "오늘 AI 판단 상한에 닿음",
        }
    }
}

impl Question {
    pub fn open(&self) -> bool {
        self.answer.is_none()
    }

    /// Open and a person's to answer: not a notice, and not a request the
    /// Observer is still sorting.
    pub fn awaits_person(&self) -> bool {
        self.open()
            && self
                .routing
                .as_ref()
                .is_none_or(|routing| routing.to != RouteTo::Pending)
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
    /// The Observer's kind and one-line reason for a decision it made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<DecisionKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl DecisionRecord {
    pub fn new(text: String, by: String, at: UnixMs) -> Self {
        Self {
            text,
            by,
            at,
            kind: None,
            reason: None,
        }
    }
}

/// `by` of a decision, an answer or a merge the Observer made (D-19).
pub const OBSERVER: &str = "observer";

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
    MergeRefused,
}

impl Gate {
    pub const ALL: [Self; 11] = [
        Self::ReviewDirectly,
        Self::ApprovedScopeChange,
        Self::BreakingChange,
        Self::NoVerification,
        Self::RiskPath,
        Self::ManualMode,
        Self::OpenQuestion,
        Self::CheckFailed,
        Self::AutonomyDiff,
        Self::DirtyMain,
        Self::MergeRefused,
    ];

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
            Self::MergeRefused => "머지가 거절됨",
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
    WorkerStart,
    PublishRefused,
    /// The worker disappeared again after its one automatic restart (D-25).
    WorkerGone,
}

impl StopReason {
    pub const ALL: [Self; 8] = [
        Self::NoReport,
        Self::Stalled,
        Self::VerifyFailed,
        Self::NewTaskCap,
        Self::EnvironmentRepeated,
        Self::WorkerStart,
        Self::PublishRefused,
        Self::WorkerGone,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::NoReport => "보고 없이 멈춤",
            Self::Stalled => "정체",
            Self::VerifyFailed => "검증 3회 실패",
            Self::NewTaskCap => "새 Task 상한",
            Self::EnvironmentRepeated => "같은 환경 실패 반복",
            Self::WorkerStart => "worker 시작 실패",
            Self::PublishRefused => "push 거절됨",
            Self::WorkerGone => "작업자 사라짐",
        }
    }
}

/// Why no new worker starts on this machine (B57).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvHold {
    /// Free disk is below the Factory's floor.
    DiskFloor,
    /// A command failed with no space left on the device.
    DiskFull,
    MemoryCritical,
}

impl EnvHold {
    pub const ALL: [Self; 3] = [Self::DiskFloor, Self::DiskFull, Self::MemoryCritical];

    pub fn label(self) -> &'static str {
        match self {
            Self::DiskFloor => "디스크 여유가 기준보다 작음",
            Self::DiskFull => "디스크 부족",
            Self::MemoryCritical => "메모리 압박 critical",
        }
    }
}

/// The worker a Task runs, as the runtime adapter reported it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRef {
    /// The Factory that spawned it.
    #[serde(default)]
    pub factory: String,
    /// The coordination agent id, which ends it.
    #[serde(default)]
    pub agent: Option<String>,
    pub name: String,
    pub pane: Option<String>,
    pub runtime: Runtime,
    pub worktree: String,
    pub branch: String,
    pub started_at: UnixMs,
    pub asleep: bool,
    /// The candidate's model and effort it started with; `None` is the
    /// CLI's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
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
    /// What the runtime said when the stop came from outside the Task, such
    /// as a refused worker start; shown beside the stop reason.
    #[serde(default)]
    pub stop_detail: Option<String>,
    /// Worker starts refused or abandoned so far; names the next start's intent.
    #[serde(default)]
    pub spawn_refusals: u32,
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
    /// The worker's last report: ask, block, propose, decide or done.
    pub last_report_at: Option<UnixMs>,
    /// When the engine last started or woke the worker; a rest that began
    /// before it is not the worker's current one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub woken_at: Option<UnixMs>,
    /// Intent keys of external writes already made (B73).
    pub writes: BTreeSet<String>,
    /// Environment hold: a start was refused by the pre-start check (B57).
    pub held: Option<String>,
    /// The same hold as a code.
    #[serde(default)]
    pub held_code: Option<EnvHold>,
    pub label_path: bool,
    pub source_body_hash: Option<String>,
    /// The intake review's candidate and its reason (D-41).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_pick: Option<WorkerPick>,
    /// The candidate the worker first started with; a retry, an automatic
    /// restart and a resume reuse it (D-42).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launched: Option<WorkerCandidate>,
    /// The wake and diagnosis of a worker resting without a report (D-22,
    /// D-23, D-35); cleared by the next report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<Recovery>,
    /// The rest start seen last and the one before it (D-24).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest_seen: Option<UnixMs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest_before: Option<UnixMs>,
    /// The Observer's one-line diagnosis of a no-report stop (B23).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnosis: Option<String>,
    /// Automatic restarts of a vanished worker since a person last started
    /// it (D-25).
    #[serde(default)]
    pub auto_restarts: u32,
    /// Why the Task is paused, when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<PauseReason>,
}

/// The intake review's candidate, by its place in the list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerPick {
    pub index: usize,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recovery {
    /// When the engine woke the worker, or decided it could not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub woke_at: Option<UnixMs>,
    /// When the diagnosis was asked, and whether its answer came.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnosed_at: Option<UnixMs>,
    #[serde(default)]
    pub diagnosing: bool,
    /// The one worker text the diagnosis read, if it had any (D-37).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnosed_from: Option<crate::judgment::WorkerTextSource>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    /// A person paused the Task.
    Person,
    /// The operator closed the worker's pane in Hide (D-26).
    PaneClosed,
}

impl PauseReason {
    pub const ALL: [Self; 2] = [Self::Person, Self::PaneClosed];
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
            stop_detail: None,
            spawn_refusals: 0,
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
            woken_at: None,
            writes: BTreeSet::new(),
            held: None,
            held_code: None,
            label_path: false,
            source_body_hash: None,
            ai_pick: None,
            launched: None,
            recovery: None,
            rest_seen: None,
            rest_before: None,
            diagnosis: None,
            auto_restarts: 0,
            pause_reason: None,
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
        if !factory.config.verification.configured() {
            return MergeMode::Manual;
        }
        self.human.merge_mode.unwrap_or(factory.config.merge_mode)
    }

    /// The candidate a new worker starts with, its place in the list when it
    /// is one, and whether a person chose it: the started one, else a
    /// person's pin, else the review's pick, else the first (D-41, D-42).
    pub fn candidate(&self, factory: &Factory) -> (WorkerCandidate, Option<usize>, bool) {
        let candidates = factory.config.candidates();
        if let Some(launched) = &self.launched {
            let index = candidates.iter().position(|c| c == launched);
            return (launched.clone(), index, true);
        }
        if let Some(index) = self.human.worker.filter(|i| *i < candidates.len()) {
            return (candidates[index].clone(), Some(index), true);
        }
        if let Some(agent) = self.human.runtime {
            return match candidates.iter().position(|c| c.agent == agent) {
                Some(index) => (candidates[index].clone(), Some(index), true),
                None => (WorkerCandidate::bare(agent), None, true),
            };
        }
        if let Some(pick) = self.ai_pick.as_ref().filter(|p| p.index < candidates.len()) {
            return (candidates[pick.index].clone(), Some(pick.index), false);
        }
        (candidates[0].clone(), Some(0), false)
    }

    /// The card a person must look at: blocked, stopped, merge waiting, or
    /// an open question (D-28, D-47).
    pub fn needs_person(&self) -> bool {
        matches!(
            self.state,
            TaskState::Blocked | TaskState::Stopped | TaskState::MergeWaiting
        ) || (self.state == TaskState::Paused && self.pause_reason == Some(PauseReason::PaneClosed))
            || self.open_questions().any(|question| {
                question.awaits_person() && !matches!(question.kind, QuestionKind::Notice)
            })
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

#[cfg(test)]
mod summary_tests {
    use super::*;

    #[test]
    fn an_old_card_skips_title_and_template_headings_and_bounds_unicode() {
        let old = serde_json::json!({"title":"알림 설정", "goal":"알림 설정\n### Goal\n**목표:**\n한 곳에서 알림을 고른다. 다음 문장은 제외한다.","criteria":[],"out_of_scope":[],"open_decisions":[],"depends_on":[],"external":[]});
        let card: Card = serde_json::from_value(old).unwrap();
        assert_eq!(card.summary(), "한 곳에서 알림을 고른다.");
        assert_eq!(
            card.summary, None,
            "reading a legacy card does not migrate it"
        );
        let long = short_summary(&"가".repeat(80));
        assert_eq!(long.chars().count(), 60);
        assert!(long.ends_with('…'));
        assert_eq!(short_summary(" two\n lines  "), "two lines");
    }
}
