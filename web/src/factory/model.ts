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
export const QUESTION_KINDS = ["intake", "split", "default", "blocking", "scope_change", "new_task_cap", "proposed_task", "action", "confirm_card", "proposal", "notice"] as const;
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

/** `contracts/snapshot-wire-enums.json`: `factory_attempt_outcome`; `running` while it has no result. */
export const ATTEMPT_OUTCOMES = ["passed", "failed", "environment", "running"] as const;
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
export const RESULT_CODES = ["wake_worker", "apply_or_merge", "ready", "split", "drafting", "new_task_cap_choice", "run_action", "acknowledge", "merge", "restart_worker", "resume_worker"] as const;
export type ResultCode = (typeof RESULT_CODES)[number];

/** `contracts/snapshot-wire-enums.json`: `factory_notice`, what a notice says. */
export const NOTICES = ["ai_answered", "ai_card_fixed", "ai_new_task", "ai_risk_merge", "daily_limit"] as const;
export type Notice = (typeof NOTICES)[number];

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

/** The inbox groups, in the order the engine ranks them. */
export const INBOX_GROUPS = ["answer", "merge", "stopped", "notice"] as const;
export type InboxGroup = (typeof INBOX_GROUPS)[number];

export type FactorySummary = {
  /** The one person-facing number: open inbox items across Factories. */
  my_turn: number;
  /** Notices to acknowledge across Factories; not part of `my_turn`. */
  notices: number;
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
  notices: number;
  observer_mode: ObserverMode;
  /** Factory AI judgments made today, local time, against `observer_limit`. */
  observer_today: number;
  observer_limit: number;
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
  /** A question kind, `merge`, `stopped`, or `paused` for a worker whose pane was closed. */
  kind: QuestionKind | "merge" | "stopped" | "paused";
  rank: number;
  factory: string;
  task: string;
  display_id: string;
  title: string;
  project: string;
  question: string | null;
  text: string;
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
  /** What a notice says. */
  notice: Notice | null;
  /** The question a notice is about; 다른 답 answers that one. */
  refers_to: string | null;
  /** How the Observer sorted this request, and its one-line reason. */
  decision_kind: DecisionKind | null;
  observer_reason: string | null;
  /** Whether Factory AI's answer can still be changed. */
  overridable: boolean;
};

export type Attachment = { path: string; sha256: string; version: number; original: string };
export type PullRequest = { number: number; url: string; head: string; by_factory: boolean; open: boolean };
export type DecisionRecord = { text: string; by: string; at: UnixMs; kind?: DecisionKind | null; reason?: string | null };
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
  /** How the Observer sorted it, and whether a person replaced its answer. */
  routing?: { kind?: DecisionKind; reason?: string; overridden?: boolean } | null;
};
export type Discovery = { id: string; class: DiscoveryClass; text: string; at: UnixMs; task: string | null };
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
  decisions: DecisionRecord[];
  questions: Question[];
  discoveries: Discovery[];
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
