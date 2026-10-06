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
export const COLUMNS = ["drafting", "waiting", "running", "done"] as const;
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

/** The inbox groups, in the order the engine ranks them. */
export const INBOX_GROUPS = ["answer", "merge", "stopped", "notice"] as const;
export type InboxGroup = (typeof INBOX_GROUPS)[number];

export type FactorySummary = {
  /** The one person-facing number: open inbox items across Factories. */
  my_turn: number;
  factories: FactoryView[];
  inbox: InboxItem[];
};

export type Flow = { drafting: number; waiting: number; running: number; done_today: number };

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
};

export type ColumnView = { column: Column; label: string; cards: CardView[] };

export type CardView = {
  task: string;
  /** `T-n` before Ready, the issue number after. */
  display_id: string;
  column: Column | null;
  title: string;
  state: TaskState;
  state_label: string;
  needs_person: boolean;
  waiting_for: string | null;
  priority: number;
  since: UnixMs;
  unread: boolean;
  folded: boolean;
  archived: boolean;
  failures: number;
  external: string[];
  revive_until: UnixMs | null;
  worker_pane: string | null;
};

export type InboxItem = {
  group: InboxGroup;
  /** A question kind, `merge` or `stopped`. */
  kind: QuestionKind | "merge" | "stopped";
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
  waiting_since: UnixMs;
  waiting_days: number;
};

export type Attachment = { path: string; sha256: string; version: number; original: string };
export type PullRequest = { number: number; url: string; head: string; by_factory: boolean; open: boolean };
export type DecisionRecord = { text: string; by: string; at: UnixMs };
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
  gates: string[];
  /** The verbs this state allows a person. */
  allowed: string[];
  stop: string | null;
  merge_sha: string | null;
  worker_name: string | null;
  worktree: string | null;
  branch: string | null;
};

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
