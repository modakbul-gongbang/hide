// A pane's relatives in the delegation tree (PRD S6 D-07, B14-B16), read from
// what the core already projects on each pane: `children.chips` for its
// direct children and `lineage_path` for its ancestors. Moving to one is a
// focus_pane that carries a request id; the core owns the outcome, so a
// click's pending, failed and retry states are its answer, never a guess.

import type { AgentChip, LineageStep, PaneFocusRequest, PaneRow } from "./snapshot";

/**
 * The one focus the shell asked for by relationship, and where it was asked
 * from. `timedOut` is set when no answer arrived in time: a send the socket
 * dropped, or a refusal that wrote no receipt, must not leave it pending.
 */
export type Relation = { requestId: string; sourcePaneId: string; targetPaneId: string; label: string; timedOut?: boolean };

/** How long a relationship focus waits for the core's answer before it reads as failed. */
export const RELATION_ANSWER_TIMEOUT_MS = 15_000;

export type RelationState = { phase: "pending" } | { phase: "failed"; message: string; retryable: boolean } | null;

/**
 * Where the request stands by the core's receipt. Until the receipt names
 * this request it is still in flight; a succeeded one needs no mark.
 */
export function relationState(relation: Relation | null, outcome: PaneFocusRequest | null | undefined): RelationState {
  if (!relation) return null;
  const answered = outcome && outcome.request_id === relation.requestId ? outcome : null;
  if (answered?.phase === "failed") {
    return { phase: "failed", message: answered.message ?? `Could not open ${relation.label}`, retryable: answered.retryable };
  }
  if (answered && answered.phase !== "pending") return null;
  if (relation.timedOut) return { phase: "failed", message: `${relation.label} did not open: Hide did not answer in time.`, retryable: true };
  return { phase: "pending" };
}

/** The step a Return goes back to: the parent, the step before this pane. */
export function parentStep(pane: PaneRow): LineageStep | null {
  const path = pane.lineage_path ?? [];
  return path.length >= 2 ? (path[path.length - 2] ?? null) : null;
}

export function directChildren(pane: PaneRow): AgentChip[] {
  return pane.children?.chips ?? [];
}

/** A chip's full name for its tooltip and accessible name: who, what state, and what it said. */
export function chipTitle(chip: AgentChip): string {
  return [chip.label, chip.status_label, chip.detail].filter(Boolean).join(" · ");
}

/** The colour class of a chip's status mark, as the native row picks it (`AgentStatusPresentation`). */
export function chipTone(chip: Pick<AgentChip, "demand" | "activity" | "emphasized">): string {
  if (chip.demand === "error") return "text-danger";
  if (chip.demand === "question" || chip.demand === "approval") return "text-warning";
  if (chip.activity === "working") return "text-agent-working";
  if (chip.activity === "stopped" && chip.emphasized) return "text-success";
  return "text-secondary";
}

export type RelationEntry = { paneId: string; label: string; relation: "parent" | "sibling" | "child"; chip: AgentChip | null };

/**
 * The relationship menu's rows (B16): the parent, the other children of that
 * parent, then this pane's own children. Each is opened only by its row's
 * explicit Open.
 */
export function relationEntries(pane: PaneRow): RelationEntry[] {
  const entries: RelationEntry[] = [];
  const parent = parentStep(pane);
  if (parent) entries.push({ paneId: parent.pane_id, label: parent.label, relation: "parent", chip: null });
  const self = pane.lineage_path?.[pane.lineage_path.length - 1];
  for (const sibling of self?.siblings ?? []) {
    if (sibling.pane_id !== pane.id) entries.push({ paneId: sibling.pane_id, label: sibling.label, relation: "sibling", chip: sibling });
  }
  for (const child of directChildren(pane)) entries.push({ paneId: child.pane_id, label: child.label, relation: "child", chip: child });
  return entries;
}
