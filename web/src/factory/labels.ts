// The words the Factory screens put on the engine's codes, in every language
// hide ships (PRD software-factory-ui B24). Each map is keyed by the full enum
// `contracts/snapshot-wire-enums.json` lists, so a value the core starts
// writing fails to compile here before it reaches a screen unlabelled.

import type { MessageKey } from "../i18n/catalogs";
import type { AttemptOutcome, AttemptStage, CardView, Column, DiscoveryClass, EnvHold, Gate, InboxGroup, InboxItem, QuestionKind, QuestionOrigin, ResultCode, StopReason, TaskState, WaitingFor } from "./model";

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

export const GROUP_LABEL: Record<InboxGroup, MessageKey> = {
  answer: "factory.group.answer",
  merge: "factory.group.merge",
  stopped: "factory.group.stopped",
  notice: "factory.group.notice",
};

export const KIND_LABEL: Record<QuestionKind | "merge" | "stopped", MessageKey> = {
  intake: "factory.kind.intake",
  split: "factory.kind.split",
  default: "factory.kind.default",
  blocking: "factory.kind.blocking",
  scope_change: "factory.kind.scope_change",
  new_task_cap: "factory.kind.new_task_cap",
  proposed_task: "factory.kind.proposed_task",
  action: "factory.kind.action",
  confirm_card: "factory.kind.confirm_card",
  proposal: "factory.kind.proposal",
  notice: "factory.kind.notice",
  merge: "factory.kind.merge",
  stopped: "factory.kind.stopped",
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

export const RESULT_LABEL: Record<ResultCode, MessageKey> = {
  wake_worker: "factory.result.wake_worker",
  apply_or_merge: "factory.result.apply_or_merge",
  ready: "factory.result.ready",
  split: "factory.result.split",
  drafting: "factory.result.drafting",
  new_task_cap_choice: "factory.result.new_task_cap_choice",
  run_action: "factory.result.run_action",
  acknowledge: "factory.result.acknowledge",
  merge: "factory.result.merge",
  restart_worker: "factory.result.restart_worker",
};

/**
 * Every reason the engine and the core refuse a command with, each with the
 * next action as a person at the screen takes it. The engine keeps no list of
 * them, so `labels.test.ts` reads every reason the Rust sources can answer
 * with and fails on one missing here.
 */
export const REFUSAL_REASONS = [
  "action_not_allowed_in_state",
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
  "task_not_found",
  "text_required",
  "verify_command_required",
] as const;
export type RefusalReason = (typeof REFUSAL_REASONS)[number];

export const REFUSAL_LABEL: Record<RefusalReason, MessageKey> = {
  action_not_allowed_in_state: "factory.refusal.action_not_allowed_in_state",
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
  task_not_found: "factory.refusal.task_not_found",
  text_required: "factory.refusal.text_required",
  verify_command_required: "factory.refusal.verify_command_required",
};

type Translate = (key: MessageKey, options?: Record<string, unknown>) => string;

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

/** Why an item is the person's (B9): a question's own words, or a merge's gates and a stop's reason from their codes. */
export function itemWhy(item: InboxItem, t: Translate): string {
  if (item.group === "merge") return item.gates.length > 0 ? t("factory.turn.mergeGates", { gates: item.gates.map((gate) => t(GATE_LABEL[gate])).join(", ") }) : t("factory.turn.mergeWhy");
  if (item.group === "stopped" && item.question === null) return t("factory.turn.stopWhy", { reason: item.stop ? t(STOP_LABEL[item.stop]) : t("factory.state.stopped") });
  return item.text;
}

/** What sending the suggestion does, and the Tasks it frees (B9). */
export function resultText(item: InboxItem, t: Translate): string {
  const result = t(RESULT_LABEL[item.result_code]);
  return item.unblocks.length > 0 ? t("factory.result.unblocks", { result, ids: item.unblocks.join(", ") }) : result;
}

/** A refusal's next action in the screen's language; an unlisted reason names itself. */
export function refusalText(reason: string | undefined, t: Translate): string {
  if (reason === undefined) return t("factory.noAnswer");
  return (REFUSAL_REASONS as readonly string[]).includes(reason) ? t(REFUSAL_LABEL[reason as RefusalReason]) : t("factory.refusal.unknown", { reason });
}
