// The words the Factory screens put on the engine's codes, in every language
// hide ships (PRD software-factory-ui B24). Each map is keyed by the full enum
// `contracts/snapshot-wire-enums.json` lists, so a value the core starts
// writing fails to compile here before it reaches a screen unlabelled.

import type { MessageKey } from "../i18n/catalogs";
import type { AttemptOutcome, AttemptStage, Column, DiscoveryClass, InboxGroup, QuestionKind, QuestionOrigin, TaskState } from "./model";

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
  drafting: "factory.column.drafting",
  waiting: "factory.column.waiting",
  running: "factory.column.running",
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
