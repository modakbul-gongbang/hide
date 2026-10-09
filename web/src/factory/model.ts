// The Factory screens' read model: the stage-1 engine's `FactorySummary` and
// `TaskDetail` as the core's `factory` and `factory_task` snapshot sections
// carry them (docs/factory.md, The read model). The shell derives nothing the
// engine did not give; every number and name on the screens comes from here.

/** Milliseconds since the Unix epoch, as the engine stamps them. */
export type UnixMs = number;

/** `contracts/snapshot-wire-enums.json`: `factory_task_state`. */
export const TASK_STATES = ["drafting", "waiting", "running", "paused", "blocked", "verifying", "merge_waiting", "landed", "done", "stopped", "relanding", "outside", "cancelled"] as const;
export type TaskState = (typeof TASK_STATES)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_column`. */
export const COLUMNS = ["before", "moving", "stuck", "done"] as const;
export type Column = (typeof COLUMNS)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_question_kind`. */
export const QUESTION_KINDS = ["intake", "split", "default", "blocking", "scope_change", "new_task_cap", "proposed_task", "action", "confirm_card"] as const;
export type QuestionKind = (typeof QUESTION_KINDS)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_question_origin`. */
export const QUESTION_ORIGINS = ["review", "worker", "check", "engine"] as const;
export type QuestionOrigin = (typeof QUESTION_ORIGINS)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_discovery_class`. */
export const DISCOVERY_CLASSES = ["in_scope", "decision", "scope_change", "prerequisite", "unrelated"] as const;
export type DiscoveryClass = (typeof DISCOVERY_CLASSES)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_attempt_stage`. */
export const ATTEMPT_STAGES = ["task", "pre_merge"] as const;
export type AttemptStage = (typeof ATTEMPT_STAGES)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_attempt_outcome`; `running` while it has no result, `cancelled` once a run was ended unanswered. */
export const ATTEMPT_OUTCOMES = ["passed", "failed", "environment", "cancelled", "running"] as const;
export type AttemptOutcome = (typeof ATTEMPT_OUTCOMES)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_waiting_for`, what a card waits for. */
export const WAITING_FOR = ["predecessors", "slot", "environment", "answer"] as const;
export type WaitingFor = (typeof WAITING_FOR)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_env_hold`, why the machine holds new starts. */
export const ENV_HOLDS = ["disk_floor", "disk_full", "memory_critical"] as const;
export type EnvHold = (typeof ENV_HOLDS)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_stop_reason`. */
export const STOP_REASONS = ["no_report", "stalled", "verify_failed", "new_task_cap", "environment_repeated", "worker_start", "publish_refused", "worker_gone"] as const;
export type StopReason = (typeof STOP_REASONS)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_gate`, why a merge waits for a person. */
export const GATES = ["review_directly", "approved_scope_change", "breaking_change", "no_verification", "risk_path", "manual_mode", "open_question", "check_failed", "autonomy_diff", "dirty_main", "merge_refused"] as const;
export type Gate = (typeof GATES)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_result_code`, what sending an item's suggestion does. */
export const RESULT_CODES = ["wake_worker", "apply_or_merge", "ready", "split", "drafting", "new_task_cap_choice", "run_action", "merge", "restart_worker", "resume_worker", "resolve"] as const;
export type ResultCode = (typeof RESULT_CODES)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_holding`, what a 결정 필요 item holds up. */
export const HOLDINGS = ["worker", "start", "merge", "progress", "starts", "github", "continues"] as const;
export type Holding = (typeof HOLDINGS)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_decision_by`, who made a decision on a Task page. */
export const DECISION_BYS = ["person", "ai", "worker"] as const;
export type DecisionBy = (typeof DECISION_BYS)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_decision_source`, where a decision came from. */
export const DECISION_SOURCES = ["answer", "assumption", "send_back", "worker", "request_changes", "risk_merge"] as const;
export type DecisionSource = (typeof DECISION_SOURCES)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_follow_up_state`. */
export const FOLLOW_UP_STATES = ["open", "issue", "factory", "discarded"] as const;
export type FollowUpState = (typeof FOLLOW_UP_STATES)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_criterion_state`, what the last check said of a criterion. */
export const CRITERION_STATES = ["met", "unmet", "unknown"] as const;
export type CriterionState = (typeof CRITERION_STATES)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_recovery_outcome`. */
export const RECOVERY_OUTCOMES = ["improved", "partial", "unchanged"] as const;
export type RecoveryOutcome = (typeof RECOVERY_OUTCOMES)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_recovery_action`, the closed list of automatic recoveries. */
export const RECOVERY_ACTIONS = ["remove_finished_worktrees", "restart_worker", "sleep_wake_worker", "switch_runtime", "retry_reads_and_reconnect"] as const;
export type RecoveryAction = (typeof RECOVERY_ACTIONS)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_decision_kind`, how the Observer sorted a request. */
export const DECISION_KINDS = ["A", "B", "C", "D", "E"] as const;
export type DecisionKind = (typeof DECISION_KINDS)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_pause_reason`, why a Task is paused. */
export const PAUSE_REASONS = ["person", "pane_closed"] as const;
export type PauseReason = (typeof PAUSE_REASONS)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_diagnosis_source`, the one worker text a diagnosis read. */
export const DIAGNOSIS_SOURCES = ["user_turn", "last_answer", "screen"] as const;
export type DiagnosisSource = (typeof DIAGNOSIS_SOURCES)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_observer_mode`, who answers a Factory's decisions. */
export const OBSERVER_MODES = ["manual", "assist", "autonomous"] as const;
export type ObserverMode = (typeof OBSERVER_MODES)[number];

/** The 결정 필요 groups, in the order the engine ranks them. */
export const INBOX_GROUPS = ["answer", "merge", "stopped", "todo"] as const;
export type InboxGroup = (typeof INBOX_GROUPS)[number];

/** A to-do's kind: a GitHub sign-in, a command a person runs, a start that never showed, a recovery that gave up. */
export const TODO_KINDS = ["github", "command", "start", "hold"] as const;
export type TodoKind = (typeof TODO_KINDS)[number];

export type FactorySummary = {
  /** The one person-facing number: every 결정 필요 item across Factories. */
  my_turn: number;
  factories: FactoryView[];
  inbox: InboxItem[];
};

export type Flow = { before: number; moving: number; stuck: number; done_today: number };

export type FactoryView = {
  id: string;
  project: string;
  project_name: string;
  source: string;
  /** `ci`, `verify` or `none`. */
  verification: string;
  closed: boolean;
  flow: Flow;
  my_turn: number;
  columns: ColumnView[];
  cancelled: CardView[];
  graph: { nodes: string[]; edges: [string, string][]; unrelated: string[] };
  /** Every dependency edge `(predecessor, task)`. */
  dependencies: [string, string][];
  outside_read_at: UnixMs | null;
  /** Three outside reads failed in a row. */
  stale: boolean;
  main_broken: boolean;
  auto_merge_available: boolean;
  merge_mode: string;
  /** The whole Factory is paused (D-48). */
  paused: boolean;
  observer_mode: ObserverMode;
  /** Factory AI judgments made today, local time, against `observer_limit`. */
  observer_today: number;
  observer_limit: number;
  /** Today's judgments reached the cap; the rest of the day goes to a person (B21). */
  observer_capped: boolean;
  /** GitHub refused the Factory's sign-in or a permission (B33). */
  github_block: GithubBlock | null;
  /** Follow-up candidates still open, newest first. */
  follow_ups: FollowUpView[];
  /** The Factory's latest activity, newest last. */
  activity: Activity[];
  /** The last seven local days' three numbers (D-45). */
  metrics: Metrics;
  /** The Factory AI's agent, model and effort; null follows Hide AI's own choice. */
  factory_ai: FactoryAi | null;
  /** Worker candidates, the first the default. */
  workers: WorkerCandidate[];
  macos_notifications: boolean;
};

export type FactoryAi = { provider: string; model?: string | null; effort?: string | null };
export type WorkerCandidate = { agent: string; model?: string | null; effort?: string | null; description: string };

export type ColumnView = { column: Column; label: string; cards: CardView[] };

export type CardView = {
  task: string;
  /** Factory AI's decisions that stand on this Task. */
  ai_decisions: number;
  /** A GitHub step waits for the Factory's access. */
  permission_wait: boolean;
  /** The recovery schedule is working on its stop. */
  recovering: boolean;
  /** `T-n` before Ready, the issue number after. */
  display_id: string;
  column: Column | null;
  title: string;
  summary: string;
  issue: string | null;
  issue_url: string | null;
  pr: PullRequest | null;
  worker_runtime: string | null;
  resume_at: UnixMs | null;
  waiting_group: "person" | "other" | null;
  stage: number;
  state: TaskState;
  state_label: string;
  needs_person: boolean;
  /** The engine's words; the screen labels `waiting_code` instead. */
  waiting_for: string | null;
  waiting_code: WaitingFor | null;
  /** The display ids of the predecessors it waits on. */
  waiting_on: string[];
  env_hold: EnvHold | null;
  /** Why a stopped card stopped. */
  stop: StopReason | null;
  priority: number;
  since: UnixMs;
  unread: boolean;
  folded: boolean;
  archived: boolean;
  failures: number;
  external: string[];
  revive_until: UnixMs | null;
  worker_pane: string | null;
  /** The running worker's agent name. */
  worker_label: string | null;
  pause_reason: PauseReason | null;
};

export type InboxItem = {
  group: InboxGroup;
  /** A question kind, `merge`, `stopped`, `paused` for a worker whose pane was closed, or a to-do's kind. */
  kind: QuestionKind | "merge" | "stopped" | "paused" | TodoKind;
  rank: number;
  factory: string;
  /** The Task it is about; a Factory's to-do has none. */
  task: string | null;
  display_id: string | null;
  title: string;
  project: string;
  question: string | null;
  /** The question or to-do in one sentence, in the asker's words; empty where only the kind says it. */
  text: string;
  /** What it holds up, as the asker wrote it. */
  stopped: string | null;
  holding: Holding;
  /** Each choice with what choosing it leads to, where the asker wrote it. */
  outcomes: ChoiceOutcome[];
  /** Why Factory AI did not decide it, or null when its kind is a person's. */
  fallback: string | null;
  /** What the folded 근거 unfolds: links, checks, the report. */
  evidence: string[];
  /** The one button's item for a to-do (`resolve`). */
  resolve: string | null;
  /** A to-do's command to copy, and what running it does. */
  command: string | null;
  impact: string | null;
  suggestion: string;
  result: string;
  default_action: string | null;
  choices: string[];
  deadline: UnixMs | null;
  remaining: string | null;
  remaining_hours: number | null;
  waiting_since: UnixMs;
  waiting_days: number;
  result_code: ResultCode;
  /** The display ids of waiting Tasks this item frees once it is done. */
  unblocks: string[];
  /** Why a merge item waits for a person. */
  gates: Gate[];
  /** Why a stopped item stopped. */
  stop: StopReason | null;
  /** Why the machine holds starts, for a hold to-do. */
  env_hold: EnvHold | null;
  /** What the recovery schedule already tried. */
  attempts: RecoveryAttempt[];
  /** How the Observer sorted this request, and its one-line reason. */
  decision_kind: DecisionKind | null;
  observer_reason: string | null;
};

export type ChoiceOutcome = { choice: string; result: string };
export type RecoveryAttempt = { at: UnixMs; action?: RecoveryAction | null; outcome?: RecoveryOutcome | null };
export type GithubBlock = { forbidden: boolean; scope?: string | null; stage: string; since: UnixMs };
export type Metrics = {
  finished: number;
  /** 결정 필요 items a person moved per finished Task, in tenths; null with nothing finished. */
  person_items_tenths: number | null;
  started: number;
  start_median_ms: number | null;
  ai_decisions: number;
  overridden: number;
  override_percent: number | null;
};
export type FollowUpView = {
  task: string;
  display_id: string;
  discovery: string;
  text: string;
  state: FollowUpState;
  issue: string | null;
  issue_url: string | null;
  became: string | null;
  /** Why the last attempt to make its issue failed. */
  failure: string | null;
  at: UnixMs;
};
export type WorkerReport = { result: string; changed?: string[]; verified?: string[]; unverified?: string[]; raw?: string | null };

/** One activity line: what happened, as a kind and its facts (docs/factory.md, The activity log). */
export type ActivityEvent =
  | { kind: "intake"; label: boolean; criteria: number; assumptions: number }
  | { kind: "started"; resumed: boolean }
  | { kind: "report"; report: WorkerReport }
  | { kind: "pull_request"; number: number; url: string }
  | { kind: "verification"; number: number; ci: boolean; outcome: "passed" | "failed" | "environment"; check?: string | null; link?: string | null }
  | { kind: "sent_back"; text: string }
  | { kind: "recovery"; action: RecoveryAction; outcome?: RecoveryOutcome | null; removed?: string[]; freed?: number | null }
  | { kind: "follow_up"; discovery: string; state: FollowUpState; issue?: string | null }
  | { kind: "ai_decision"; text: string }
  | { kind: "outside"; what: "closing_pr" | "pr_merged" | "issue_closed" | "issue_reopened"; link?: string | null }
  | { kind: "main_broken"; by_factory: boolean; link?: string | null }
  | { kind: "cleanup_kept"; worktree: string; detail: string }
  | { kind: "watch"; text: string; action?: RecoveryAction | null }
  | { kind: "daily_limit"; limit: number }
  | { kind: "note"; text: string };
export type Activity = { at: UnixMs; task?: string | null } & ActivityEvent;

export type Attachment = { path: string; sha256: string; version: number; original: string };
export type PullRequest = { number: number; url: string; head: string; by_factory: boolean; open: boolean };
export type DecisionView = {
  /** `R<n>`, what a 다른 답 names. */
  id: string;
  text: string;
  by: DecisionBy;
  recorded_by: string;
  source: DecisionSource | null;
  kind: DecisionKind | null;
  reason: string | null;
  at: UnixMs;
  /** Factory AI's, and the Task is not finished. */
  overridable: boolean;
  /** What Factory AI had decided, once a person changed it. */
  changed: { by: string; at: UnixMs; from: string } | null;
};
export type CriterionView = { text: string; state: CriterionState | null; reason: string | null };
export type Answer = { text: string; chose: string | null; relayed_by: string; at: UnixMs };
export type Question = {
  id: string;
  origin: QuestionOrigin;
  kind: { kind: QuestionKind } & Record<string, unknown>;
  text: string;
  suggestion: string;
  default_action: string | null;
  deadline: UnixMs | null;
  asked_at: UnixMs;
  choices: string[];
  answer: Answer | null;
  letter: string | null;
  /** A worker's question as Factory AI rewrote it for a person; absent when it already read so. */
  person_text?: string | null;
  /** How the Observer sorted it, and whether a person replaced its answer. */
  routing?: { kind?: DecisionKind; reason?: string; overridden?: boolean } | null;
};
export type Discovery = { id: string; class: DiscoveryClass; text: string; at: UnixMs; task: string | null; follow_up?: { state: FollowUpState } | null };
export type AttemptView = {
  number: number;
  stage: AttemptStage;
  started_at: UnixMs;
  outcome: AttemptOutcome;
  check: string | null;
  link: string | null;
  log_tail: string | null;
};

export type TaskDetail = {
  card: CardView;
  factory: string;
  project: string;
  goal: string;
  criteria: string[];
  out_of_scope: string[];
  before: string[];
  after: string[];
  attachments: Attachment[];
  pr: PullRequest | null;
  /** `n/3`, or the engine's no-verification words. */
  verification: string;
  attempts: AttemptView[];
  decisions: DecisionView[];
  questions: Question[];
  discoveries: Discovery[];
  /** Each completion criterion with what the last check said. */
  checklist: CriterionView[];
  /** The worker's last report. */
  report: WorkerReport | null;
  /** The Task's activity, oldest first. */
  activity: Activity[];
  follow_ups: FollowUpView[];
  /** The issue as written, folded at the foot; none without an issue. */
  issue_text: string | null;
  /** The engine's words; the screen labels `gate_codes` instead. */
  gates: string[];
  gate_codes: Gate[];
  /** The verbs this state allows a person. */
  allowed: string[];
  /** The engine's words; the screen labels `stop_code` instead. */
  stop: string | null;
  stop_code: StopReason | null;
  merge_sha: string | null;
  worker_name: string | null;
  worktree: string | null;
  branch: string | null;
  /** Which candidate runs the worker, and why Factory AI picked it. */
  worker: WorkerLine | null;
  /** Factory AI's one-line reading of a stopped worker. */
  diagnosis: string | null;
  auto_restarts: number;
  /** Since when the running worker rests without a report. */
  resting_since: UnixMs | null;
  /** The pinned candidate's number, 1 first; null lets Factory AI pick. */
  pinned_worker: number | null;
  /** The candidate the review picked, 1 first, and why. */
  ai_picked_worker: number | null;
  ai_pick_reason: string | null;
  /** When the engine woke the resting worker, and asked Factory AI why it rests. */
  woke_at: UnixMs | null;
  diagnosed_at: UnixMs | null;
  diagnosed_from: DiagnosisSource | null;
};

export type WorkerLine = { agent: string; label: string; model: string | null; effort: string | null; picked: string | null; pick_reason: string | null };

/** The engine's answer to one screen request: `ok`, or a refusal's `reason`, `next_action` and `detail`. */
export type ActionAnswer = { request_id: string; answer: { ok: boolean; reason?: string; next_action?: string } & Record<string, unknown> };

/** The `factory` snapshot section. */
export type FactorySection = {
  /** Absent until the engine first answers; the screen draws its skeleton meanwhile. */
  summary: FactorySummary | null;
  /** The latest answers to this screen's requests, newest last. */
  actions: ActionAnswer[];
};

/** The `factory_task` snapshot section: the Task page a screen opened. */
export type FactoryTaskSection = { factory: string; task: string; detail: TaskDetail | null };
