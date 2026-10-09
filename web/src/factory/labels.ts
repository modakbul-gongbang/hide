// The words the Factory screens put on the engine's codes, in every language
// hide ships (PRD software-factory-ui B24). Each map is keyed by the full enum
// `contracts/snapshot-wire-enums.json` lists, so a value the core starts
// writing fails to compile here before it reaches a screen unlabelled.

import type { MessageKey } from "../i18n/catalogs";
import type { Activity, AttemptOutcome, AttemptStage, CardView, Column, CriterionState, DecisionKind, DecisionSource, DiagnosisSource, DiscoveryClass, EnvHold, FollowUpState, Gate, Holding, InboxItem, ObserverMode, PauseReason, QuestionOrigin, RecoveryAction, RecoveryOutcome, StopReason, TaskState, WaitingFor } from "./model";

export const STATE_LABEL: Record<TaskState, MessageKey> = {
  drafting: "factory.state.drafting",
  waiting: "factory.state.waiting",
  running: "factory.state.running",
  paused: "factory.state.paused",
  blocked: "factory.state.blocked",
  verifying: "factory.state.verifying",
  merge_waiting: "factory.state.merge_waiting",
  landed: "factory.state.landed",
  done: "factory.state.done",
  stopped: "factory.state.stopped",
  relanding: "factory.state.relanding",
  outside: "factory.state.outside",
  cancelled: "factory.state.cancelled",
};

export const COLUMN_LABEL: Record<Column, MessageKey> = {
  before: "factory.column.before",
  moving: "factory.column.moving",
  stuck: "factory.column.stuck",
  done: "factory.column.done",
};

export const KIND_LABEL: Record<InboxItem["kind"], MessageKey> = {
  intake: "factory.kind.intake",
  split: "factory.kind.split",
  default: "factory.kind.default",
  blocking: "factory.kind.blocking",
  scope_change: "factory.kind.scope_change",
  new_task_cap: "factory.kind.new_task_cap",
  proposed_task: "factory.kind.proposed_task",
  action: "factory.kind.action",
  confirm_card: "factory.kind.confirm_card",
  merge: "factory.kind.merge",
  stopped: "factory.kind.stopped",
  paused: "factory.kind.paused",
  github: "factory.kind.github",
  command: "factory.kind.command",
  start: "factory.kind.start",
  hold: "factory.kind.hold",
};

export const ORIGIN_LABEL: Record<QuestionOrigin, MessageKey> = {
  review: "factory.origin.review",
  worker: "factory.origin.worker",
  check: "factory.origin.check",
  engine: "factory.origin.engine",
};

export const DISCOVERY_LABEL: Record<DiscoveryClass, MessageKey> = {
  in_scope: "factory.discovery.in_scope",
  decision: "factory.discovery.decision",
  scope_change: "factory.discovery.scope_change",
  prerequisite: "factory.discovery.prerequisite",
  unrelated: "factory.discovery.unrelated",
};

export const STAGE_LABEL: Record<AttemptStage, MessageKey> = {
  task: "factory.attemptStage.task",
  pre_merge: "factory.attemptStage.pre_merge",
};

export const OUTCOME_LABEL: Record<AttemptOutcome, MessageKey> = {
  passed: "factory.outcome.passed",
  failed: "factory.outcome.failed",
  environment: "factory.outcome.environment",
  cancelled: "factory.outcome.cancelled",
  running: "factory.outcome.running",
};

/** The person's actions a Task page and a merge or stop item offer, by the engine's verb. */
export const ACTION_LABEL: Record<string, MessageKey> = {
  answer: "factory.action.answer",
  edit: "factory.action.edit",
  cancel: "factory.action.cancel",
  priority: "factory.action.priority",
  "dep-remove": "factory.action.depRemove",
  pause: "factory.action.pause",
  resume: "factory.action.resume",
  retry: "factory.action.retry",
  merge: "factory.action.merge",
  "request-changes": "factory.action.requestChanges",
  revive: "factory.action.revive",
};

/** An action's words; resuming a Task paused by a closed pane starts its worker again (D-26), so it reads 다시 시작. */
export function actionKey(value: string, paused = false): MessageKey {
  return paused && value === "resume" ? "factory.resume" : (ACTION_LABEL[value] ?? "factory.turn.send");
}

/** How a state reads at a glance: the person's turn in the warning tone, work in the working tone, finished work as done. */
export type StateTone = "turn" | "working" | "done" | "quiet";

export function stateTone(state: TaskState, needsPerson: boolean): StateTone {
  if (needsPerson || state === "blocked" || state === "stopped" || state === "merge_waiting") return "turn";
  if (state === "running" || state === "verifying" || state === "relanding" || state === "outside") return "working";
  if (state === "done" || state === "landed") return "done";
  return "quiet";
}

export const TONE_TEXT: Record<StateTone, string> = {
  turn: "text-warning",
  working: "text-agent-working",
  done: "text-success",
  quiet: "text-muted-foreground",
};

export const WAITING_LABEL: Record<Exclude<WaitingFor, "predecessors" | "environment">, MessageKey> = {
  slot: "factory.waiting.slot",
  answer: "factory.waiting.answer",
};

export const ENV_HOLD_LABEL: Record<EnvHold, MessageKey> = {
  disk_floor: "factory.env.disk_floor",
  disk_full: "factory.env.disk_full",
  memory_critical: "factory.env.memory_critical",
};

export const STOP_LABEL: Record<StopReason, MessageKey> = {
  no_report: "factory.stop.no_report",
  stalled: "factory.stop.stalled",
  verify_failed: "factory.stop.verify_failed",
  new_task_cap: "factory.stop.new_task_cap",
  environment_repeated: "factory.stop.environment_repeated",
  worker_start: "factory.stop.worker_start",
  publish_refused: "factory.stop.publish_refused",
  worker_gone: "factory.stop.worker_gone",
};

export const GATE_LABEL: Record<Gate, MessageKey> = {
  review_directly: "factory.gate.review_directly",
  approved_scope_change: "factory.gate.approved_scope_change",
  breaking_change: "factory.gate.breaking_change",
  no_verification: "factory.gate.no_verification",
  risk_path: "factory.gate.risk_path",
  manual_mode: "factory.gate.manual_mode",
  open_question: "factory.gate.open_question",
  check_failed: "factory.gate.check_failed",
  autonomy_diff: "factory.gate.autonomy_diff",
  dirty_main: "factory.gate.dirty_main",
  merge_refused: "factory.gate.merge_refused",
};

/** What a 결정 필요 item holds up, said when the asker did not write it (D-33). */
export const HOLDING_LABEL: Record<Holding, MessageKey> = {
  worker: "factory.holding.worker",
  start: "factory.holding.start",
  merge: "factory.holding.merge",
  progress: "factory.holding.progress",
  starts: "factory.holding.starts",
  github: "factory.holding.github",
  continues: "factory.holding.continues",
};

/** What happens while an item waits for a person, by what it holds up. */
export const MEANWHILE_LABEL: Record<Holding, MessageKey> = {
  worker: "factory.meanwhile.worker",
  start: "factory.meanwhile.start",
  merge: "factory.meanwhile.merge",
  progress: "factory.meanwhile.progress",
  starts: "factory.meanwhile.starts",
  github: "factory.meanwhile.github",
  continues: "factory.meanwhile.continues",
};

/** Why Factory AI left a decision to a person (B7); the engine's `fallback` codes. */
export const FALLBACK_REASONS = ["failed", "daily_limit", "paused", "queue_full", "dropped", "restart"] as const;
export type FallbackReason = (typeof FALLBACK_REASONS)[number];
export const FALLBACK_LABEL: Record<FallbackReason, MessageKey> = {
  failed: "factory.fallback.failed",
  daily_limit: "factory.fallback.daily_limit",
  paused: "factory.fallback.paused",
  queue_full: "factory.fallback.queue_full",
  dropped: "factory.fallback.dropped",
  restart: "factory.fallback.restart",
};

export const DECISION_SOURCE_LABEL: Record<DecisionSource, MessageKey> = {
  answer: "factory.source.answer",
  assumption: "factory.source.assumption",
  send_back: "factory.source.send_back",
  worker: "factory.source.worker",
  request_changes: "factory.source.request_changes",
  risk_merge: "factory.source.risk_merge",
};

export const CRITERION_LABEL: Record<CriterionState, MessageKey> = {
  met: "factory.criterion.met",
  unmet: "factory.criterion.unmet",
  unknown: "factory.criterion.unknown",
};

export const FOLLOW_UP_STATE_LABEL: Record<FollowUpState, MessageKey> = {
  open: "factory.followUp.state.open",
  issue: "factory.followUp.state.issue",
  factory: "factory.followUp.state.factory",
  discarded: "factory.followUp.state.discarded",
};

/** The closed list of automatic recoveries, as the settings name them. */
export const RECOVERY_ACTION_LABEL: Record<RecoveryAction, MessageKey> = {
  remove_finished_worktrees: "factory.settings.recovery.remove_finished_worktrees",
  restart_worker: "factory.settings.recovery.restart_worker",
  sleep_wake_worker: "factory.settings.recovery.sleep_wake_worker",
  switch_runtime: "factory.settings.recovery.switch_runtime",
  retry_reads_and_reconnect: "factory.settings.recovery.retry_reads_and_reconnect",
};

export const RECOVERY_OUTCOME_LABEL: Record<RecoveryOutcome, MessageKey> = {
  improved: "factory.recovery.improved",
  partial: "factory.recovery.partial",
  unchanged: "factory.recovery.unchanged",
};

/** The Observer's five kinds of decision request (D-14). */
export const DECISION_KIND_LABEL: Record<DecisionKind, MessageKey> = {
  A: "factory.decision.A",
  B: "factory.decision.B",
  C: "factory.decision.C",
  D: "factory.decision.D",
  E: "factory.decision.E",
};

/** What Factory AI read to diagnose a quiet worker, said after its diagnosis (D-37). */
export const DIAGNOSIS_SOURCE_LABEL: Record<DiagnosisSource, MessageKey> = {
  user_turn: "factory.diagnosisSource.user_turn",
  last_answer: "factory.diagnosisSource.last_answer",
  screen: "factory.diagnosisSource.screen",
};

export const PAUSE_REASON_LABEL: Record<PauseReason, MessageKey> = {
  person: "factory.pauseReason.person",
  pane_closed: "factory.pauseReason.pane_closed",
};

/** The three choices of who answers (D-03): 직접, 함께, 맡김. */
export const MODE_LABEL: Record<ObserverMode, MessageKey> = {
  manual: "factory.mode.manual",
  assist: "factory.mode.assist",
  autonomous: "factory.mode.autonomous",
};

export const MODE_LINE: Record<ObserverMode, MessageKey> = {
  manual: "factory.mode.line.manual",
  assist: "factory.mode.line.assist",
  autonomous: "factory.mode.line.autonomous",
};

/** Why a decision the Observer sorted is still a person's, in the Factory's mode. */
export const MODE_MINE: Record<ObserverMode, MessageKey> = {
  manual: "factory.decision.mine.manual",
  assist: "factory.decision.mine.assist",
  autonomous: "factory.decision.mine.autonomous",
};

/**
 * Every reason the engine and the core refuse a command with, each with the
 * next action as a person at the screen takes it. The engine keeps no list of
 * them, so `labels.test.ts` reads every reason the Rust sources can answer
 * with and fails on one missing here.
 */
export const REFUSAL_REASONS = [
  "action_not_allowed_in_state",
  "agent_not_installed",
  "agent_not_startable",
  "already_answered",
  "answer_required",
  "attachment_failed",
  "auto_needs_verification",
  "capacity",
  "card_invalid",
  "choice_invalid",
  "ci_checks_required",
  "ci_unavailable",
  "comment_required",
  "config_invalid",
  "deadline_required",
  "default_required",
  "dependency_cycle",
  "discovery_not_found",
  "factory_agent_unbound",
  "factory_ai_required",
  "factory_ai_unavailable",
  "factory_ambiguous",
  "factory_closed",
  "factory_has_running_tasks",
  "factory_not_found",
  "factory_screen_verb",
  "factory_busy",
  "factory_unavailable",
  "github_login_required",
  "github_permission_missing",
  "github_unavailable",
  "init_failed",
  "instruction_required",
  "issue_invalid",
  "letter_invalid",
  "letter_unavailable",
  "lineage_unknown",
  "main_dirty",
  "merge_conflict",
  "merge_failed",
  "new_task_limit",
  "no_open_question",
  "out_of_range",
  "project_required",
  "proposal_depth_exceeded",
  "question_required",
  "quick_check_failed",
  "reclassify_away_from_person",
  "report_limit",
  "revive_expired",
  "sender_not_a_worker",
  "suggestion_required",
  "task_ambiguous",
  "task_cancelled",
  "task_finished",
  "task_not_found",
  "text_required",
  "verify_command_required",
  "worker_description_too_long",
  "worker_out_of_range",
  "decision_not_changeable",
  "decision_not_found",
  "follow_up_failed",
  "follow_up_not_found",
  "follow_up_settled",
  "github_blocked",
  "github_still_blocked",
  "item_not_found",
  "result_required",
  "summary_replaced",
] as const;
export type RefusalReason = (typeof REFUSAL_REASONS)[number];

export const REFUSAL_LABEL: Record<RefusalReason, MessageKey> = {
  action_not_allowed_in_state: "factory.refusal.action_not_allowed_in_state",
  agent_not_installed: "factory.refusal.agent_not_installed",
  agent_not_startable: "factory.refusal.agent_not_startable",
  already_answered: "factory.refusal.already_answered",
  answer_required: "factory.refusal.answer_required",
  attachment_failed: "factory.refusal.attachment_failed",
  auto_needs_verification: "factory.refusal.auto_needs_verification",
  capacity: "factory.refusal.capacity",
  card_invalid: "factory.refusal.card_invalid",
  choice_invalid: "factory.refusal.choice_invalid",
  ci_checks_required: "factory.refusal.ci_checks_required",
  ci_unavailable: "factory.refusal.ci_unavailable",
  comment_required: "factory.refusal.comment_required",
  config_invalid: "factory.refusal.config_invalid",
  deadline_required: "factory.refusal.deadline_required",
  default_required: "factory.refusal.default_required",
  dependency_cycle: "factory.refusal.dependency_cycle",
  discovery_not_found: "factory.refusal.discovery_not_found",
  factory_agent_unbound: "factory.refusal.factory_agent_unbound",
  factory_ai_required: "factory.refusal.factory_ai_required",
  factory_ai_unavailable: "factory.refusal.factory_ai_unavailable",
  factory_ambiguous: "factory.refusal.factory_ambiguous",
  factory_closed: "factory.refusal.factory_closed",
  factory_has_running_tasks: "factory.refusal.factory_has_running_tasks",
  factory_not_found: "factory.refusal.factory_not_found",
  factory_screen_verb: "factory.refusal.factory_screen_verb",
  factory_busy: "factory.refusal.factory_busy",
  factory_unavailable: "factory.refusal.factory_unavailable",
  github_login_required: "factory.refusal.github_login_required",
  github_permission_missing: "factory.refusal.github_permission_missing",
  github_unavailable: "factory.refusal.github_unavailable",
  init_failed: "factory.refusal.init_failed",
  instruction_required: "factory.refusal.instruction_required",
  issue_invalid: "factory.refusal.issue_invalid",
  letter_invalid: "factory.refusal.letter_invalid",
  letter_unavailable: "factory.refusal.letter_unavailable",
  lineage_unknown: "factory.refusal.lineage_unknown",
  main_dirty: "factory.refusal.main_dirty",
  merge_conflict: "factory.refusal.merge_conflict",
  merge_failed: "factory.refusal.merge_failed",
  new_task_limit: "factory.refusal.new_task_limit",
  no_open_question: "factory.refusal.no_open_question",
  out_of_range: "factory.refusal.out_of_range",
  project_required: "factory.refusal.project_required",
  proposal_depth_exceeded: "factory.refusal.proposal_depth_exceeded",
  question_required: "factory.refusal.question_required",
  quick_check_failed: "factory.refusal.quick_check_failed",
  reclassify_away_from_person: "factory.refusal.reclassify_away_from_person",
  report_limit: "factory.refusal.report_limit",
  revive_expired: "factory.refusal.revive_expired",
  sender_not_a_worker: "factory.refusal.sender_not_a_worker",
  suggestion_required: "factory.refusal.suggestion_required",
  task_ambiguous: "factory.refusal.task_ambiguous",
  task_cancelled: "factory.refusal.task_cancelled",
  task_finished: "factory.refusal.task_finished",
  task_not_found: "factory.refusal.task_not_found",
  text_required: "factory.refusal.text_required",
  verify_command_required: "factory.refusal.verify_command_required",
  worker_description_too_long: "factory.refusal.worker_description_too_long",
  worker_out_of_range: "factory.refusal.worker_out_of_range",
  decision_not_changeable: "factory.refusal.decision_not_changeable",
  decision_not_found: "factory.refusal.decision_not_found",
  follow_up_failed: "factory.refusal.follow_up_failed",
  follow_up_not_found: "factory.refusal.follow_up_not_found",
  follow_up_settled: "factory.refusal.follow_up_settled",
  github_blocked: "factory.refusal.github_blocked",
  github_still_blocked: "factory.refusal.github_still_blocked",
  item_not_found: "factory.refusal.item_not_found",
  result_required: "factory.refusal.result_required",
  summary_replaced: "factory.refusal.summary_replaced",
};

export type Translate = (key: MessageKey, options?: Record<string, unknown>) => string;

/** What a waiting or blocked card waits for, from its code (B15, B17). */
export function waitingText(card: CardView, t: Translate): string | null {
  switch (card.waiting_code) {
    case null:
      return null;
    case "predecessors":
      return card.waiting_on.length > 0 ? t("factory.waiting.predecessors", { ids: card.waiting_on.join(", ") }) : t("factory.waiting.predecessor");
    case "environment":
      return card.env_hold ? t("factory.waiting.environment", { reason: t(ENV_HOLD_LABEL[card.env_hold]) }) : t("factory.waiting.environmentAny");
    default:
      return t(WAITING_LABEL[card.waiting_code]);
  }
}

/** Why a request the Observer sorted is the person's: its kind, and who decides it in this mode (D-14, D-21). */
export function decisionWhy(kind: DecisionKind, mode: ObserverMode | null, t: Translate): string {
  const who = kind === "D" ? t("factory.decision.permission") : mode ? t(MODE_MINE[mode]) : null;
  return who ? `${t(DECISION_KIND_LABEL[kind])} · ${who}` : t(DECISION_KIND_LABEL[kind]);
}

/** A refusal's next action in the screen's language; an unlisted reason names itself. */
export function refusalText(reason: string | undefined, t: Translate): string {
  if (reason === undefined) return t("factory.noAnswer");
  return (REFUSAL_REASONS as readonly string[]).includes(reason) ? t(REFUSAL_LABEL[reason as RefusalReason]) : t("factory.refusal.unknown", { reason });
}

/** A recovery action's short name, as an activity line or a 결정 필요 item's 자동 복구 row says it. */
export const RECOVERY_SHORT: Record<RecoveryAction, MessageKey> = {
  remove_finished_worktrees: "factory.recovery.action.remove_finished_worktrees",
  restart_worker: "factory.recovery.action.restart_worker",
  sleep_wake_worker: "factory.recovery.action.sleep_wake_worker",
  switch_runtime: "factory.recovery.action.switch_runtime",
  retry_reads_and_reconnect: "factory.recovery.action.retry_reads_and_reconnect",
};

const OUTSIDE_LABEL: Record<Extract<Activity, { kind: "outside" }>["what"], MessageKey> = {
  closing_pr: "factory.activity.outside.closing_pr",
  pr_merged: "factory.activity.outside.pr_merged",
  issue_closed: "factory.activity.outside.issue_closed",
  issue_reopened: "factory.activity.outside.issue_reopened",
};

const VERIFICATION_LABEL: Record<Extract<Activity, { kind: "verification" }>["outcome"], MessageKey> = {
  passed: "factory.outcome.passed",
  failed: "factory.outcome.failed",
  environment: "factory.outcome.environment",
};

/** Bytes as the screen says a size: GB with one decimal, else MB. */
export function bytesText(bytes: number): string {
  return bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(1)}GB` : `${Math.max(1, Math.round(bytes / 1024 ** 2))}MB`;
}

/**
 * One activity line in the operator's language: its sentence and, where it
 * has one, the smaller line under it. Free text inside is the worker's, the
 * AI's or a migrated notice's own words.
 */
export function activityText(entry: Activity, t: Translate): { text: string; detail: string | null } {
  switch (entry.kind) {
    case "intake":
      return {
        text: entry.label ? t("factory.activity.intakeLabel") : t("factory.activity.intake"),
        detail: t("factory.activity.intakeDetail", { criteria: entry.criteria, assumptions: entry.assumptions }),
      };
    case "started":
      return { text: entry.resumed ? t("factory.activity.resumed") : t("factory.activity.started"), detail: null };
    case "report":
      return { text: t("factory.activity.report"), detail: null };
    case "pull_request":
      return { text: t("factory.activity.pullRequest", { number: entry.number }), detail: null };
    case "verification": {
      const outcome = t(VERIFICATION_LABEL[entry.outcome]);
      return { text: entry.ci ? t("factory.activity.ci", { check: entry.check ?? "", outcome }) : t("factory.activity.verify", { number: entry.number, outcome }), detail: null };
    }
    case "sent_back":
      return { text: t("factory.activity.sentBack", { text: entry.text }), detail: null };
    case "recovery": {
      const action = t(RECOVERY_SHORT[entry.action]);
      const freed = entry.freed ? t("factory.activity.freed", { size: bytesText(entry.freed) }) : null;
      const removed = entry.removed && entry.removed.length > 0 ? t("factory.activity.removed", { count: entry.removed.length }) : null;
      return {
        text: entry.outcome ? t("factory.activity.recovery", { action, outcome: t(RECOVERY_OUTCOME_LABEL[entry.outcome]) }) : t("factory.activity.recovering", { action }),
        detail: [removed, freed].filter((part) => part !== null).join(" · ") || null,
      };
    }
    case "follow_up":
      return { text: t("factory.activity.followUp", { state: t(FOLLOW_UP_STATE_LABEL[entry.state]) }), detail: entry.issue ?? null };
    case "ai_decision":
      return { text: t("factory.activity.aiDecision", { text: entry.text }), detail: null };
    case "outside":
      return { text: t(OUTSIDE_LABEL[entry.what]), detail: null };
    case "main_broken":
      return { text: entry.by_factory ? t("factory.activity.mainBrokenByFactory") : t("factory.activity.mainBrokenOutside"), detail: null };
    case "cleanup_kept":
      return { text: t("factory.activity.cleanupKept", { worktree: entry.worktree.split("/").pop() ?? entry.worktree }), detail: entry.detail };
    case "watch":
      return { text: t("factory.activity.watch", { text: entry.text }), detail: entry.action ? t(RECOVERY_SHORT[entry.action]) : null };
    case "daily_limit":
      return { text: t("factory.activity.dailyLimit", { limit: entry.limit }), detail: null };
    case "note":
      return { text: entry.text, detail: null };
  }
}
