// The close flow: a pane whose activity is unknown
// is not closed until status is refreshed; a pane with working or attention
// state asks once; an idle pane closes immediately with `confirmed: false`.
// The decision is pure so the tab bar, the pane header and the shortcut
// registry share it and a test can price each branch (PRD S2 B7).

import type { AgentRow, AsyncOperation, PaneRow } from "./snapshot";

export type CloseKind = "pane" | "tab";

export type CloseDecision =
  | { action: "status_unknown"; label: string }
  | { action: "confirm" }
  | { action: "close" };

type Target = { label: string; confirmation: boolean; statusCheck: boolean };

function target(pane: PaneRow, agents: readonly AgentRow[]): Target {
  const agent = agents.find((row) => row.pane_id === pane.id);
  return {
    label: pane.herdr_label ?? pane.id,
    confirmation: pane.requires_close_confirmation || (agent?.requires_close_confirmation ?? false),
    statusCheck: pane.requires_close_status_check || (agent?.requires_close_status_check ?? false),
  };
}

export function closeDecision(panes: PaneRow[], agents: AgentRow[]): CloseDecision {
  const targets = panes.map((pane) => target(pane, agents));
  const unknown = targets.find((row) => row.statusCheck);
  if (unknown) return { action: "status_unknown", label: unknown.label };
  return targets.some((row) => row.confirmation) ? { action: "confirm" } : { action: "close" };
}

/** The Stop-work sheet's words, which stay as they are while it is open (B28). */
export function stopWorkCopy(kind: CloseKind): { title: string; consequence: string } {
  return kind === "pane"
    ? { title: "Stop the active pane?", consequence: "Closing this pane terminates its running process and interrupts the listed work." }
    : { title: "Close this tab?", consequence: "Closing the tab terminates all listed working or attention panes in one operation." };
}

/** A pane the Stop-work sheet lists: working or asking (`active`), unreadable, or quiet. */
export type StopWorkRow = { pane: PaneRow; agent: AgentRow | null; label: string; state: "active" | "unknown" | "quiet" };

export type StopWork = {
  rows: StopWorkRow[];
  /** A pane's activity is unknown, so Stop work and close waits for a status check (B28). */
  unknown: StopWorkRow | null;
};

/**
 * The open Stop-work sheet's list, re-derived from every snapshot (PRD
 * close-agent-subtree B28, D-39): every pane that closes, each with its mark
 * and status word, a quiet one dimmed rather than dropped, so a pane that
 * settles or starts while the sheet is open changes in place.
 */
export function stopWorkOf(panes: readonly PaneRow[], agents: readonly AgentRow[]): StopWork {
  const rows = panes.map((pane): StopWorkRow => {
    const row = target(pane, agents);
    const agent = agents.find((candidate) => candidate.pane_id === pane.id) ?? null;
    const state = row.statusCheck ? "unknown" : row.confirmation ? "active" : "quiet";
    // Named as the sidebar and the pane header name it, not by its pane id.
    return { pane, agent, label: agent?.identity_label ?? pane.identity_label ?? row.label, state };
  });
  return { rows, unknown: rows.find((row) => row.state === "unknown") ?? null };
}

/** The notice for an unknown activity status; `refresh_status` is the way out. */
export function statusUnknownNotice(label: string): string {
  return `Activity status for ${label} is unknown. Check status before closing.`;
}

/**
 * How a row in a close list reads to the operator (PRD close-agent-subtree
 * D-33): working, waiting for an answer (a question, approval or error),
 * a finished result not yet looked at, an activity Hide cannot read, or
 * quiet (idle, a read result, asleep).
 */
export type SubtreeState = "working" | "waiting" | "unread" | "unknown" | "quiet";

export type SubtreeRow = {
  agent: AgentRow;
  /** How far under the closed agent it sits; the closed agent itself is 0. */
  depth: number;
  /** The agent being closed (or, for a removal, one inside what is removed); not counted in N. */
  target: boolean;
  state: SubtreeState;
};

export type Subtree = {
  /** The descendant panes to close, exactly what the list shows. */
  ids: string[];
  rows: SubtreeRow[];
  counts: Record<Exclude<SubtreeState, "quiet">, number>;
  /** A listed descendant's activity is unknown, so closing it too waits for a status check. */
  unknown: boolean;
};

export function subtreeState(agent: AgentRow): SubtreeState {
  // The core's own close guard, so the sheet blocks exactly what the core
  // would refuse (B10, D-22).
  if (agent.requires_close_status_check) return "unknown";
  if (agent.demand && agent.demand !== "none") return "waiting";
  if (agent.activity === "working") return "working";
  if (agent.unread && agent.symbol === "✓") return "unread";
  return "quiet";
}

/**
 * What closing the panes in `inside` would leave running: every live
 * descendant of an agent there that is not itself there (D-26), from the
 * core's own close lists, which leave out a device that is not connected
 * (D-16). `agents` is every current agent row, this machine's and each
 * connected device's. Null when nothing would be left behind, which keeps
 * the ordinary close (B1).
 */
export function subtreeOf(inside: readonly string[], agents: readonly AgentRow[]): Subtree | null {
  const within = new Set(inside);
  const byPane = new Map(agents.map((agent) => [agent.pane_id, agent]));
  const ids: string[] = [];
  for (const agent of agents) {
    if (!within.has(agent.pane_id)) continue;
    for (const pane of agent.close_descendant_pane_ids ?? []) {
      if (within.has(pane) || !byPane.has(pane) || ids.includes(pane)) continue;
      ids.push(pane);
    }
  }
  if (ids.length === 0) return null;
  const listed = new Set(ids);
  const rows: SubtreeRow[] = [];
  const shown = new Set<string>();
  const visit = (agent: AgentRow, rootDepth: number) => {
    for (const child of agent.lineage_child_pane_ids ?? []) {
      const row = byPane.get(child);
      if (!row || shown.has(child)) continue;
      if (listed.has(child)) {
        shown.add(child);
        rows.push({ agent: row, depth: Math.max(1, (row.lineage_depth ?? 0) - rootDepth), target: false, state: subtreeState(row) });
      }
      visit(row, rootDepth);
    }
  };
  for (const agent of agents) {
    if (!within.has(agent.pane_id) || !(agent.close_descendant_pane_ids ?? []).some((pane) => listed.has(pane))) continue;
    rows.push({ agent, depth: 0, target: true, state: subtreeState(agent) });
    visit(agent, agent.lineage_depth ?? 0);
  }
  // A listed descendant the walk did not reach still shows, so the list is
  // never shorter than what closes.
  for (const pane of ids) {
    const row = byPane.get(pane);
    if (row && !shown.has(pane)) rows.push({ agent: row, depth: 1, target: false, state: subtreeState(row) });
  }
  const counts = { working: 0, waiting: 0, unread: 0, unknown: 0 };
  for (const row of rows) if (!row.target && row.state !== "quiet") counts[row.state] += 1;
  return { ids, rows, counts, unknown: counts.unknown > 0 };
}

/** Which sheet an open close shows. */
export type CloseSheet = { sheet: "subtree"; subtree: Subtree } | { sheet: "stop_work"; stopWork: StopWork };

/**
 * Which sheet an open close shows now (B28, D-40): the subtree sheet while
 * the target has a live descendant outside it, the target's Stop-work sheet
 * otherwise, so a descendant that appears or the last one that leaves turns
 * one into the other in place. `hostAgents` are the agent rows of the
 * target's own host; `everyAgent` is every listed agent row.
 */
export function closeSheet(panes: readonly PaneRow[], hostAgents: readonly AgentRow[], everyAgent: readonly AgentRow[]): CloseSheet {
  const subtree = subtreeOf(panes.map((pane) => pane.id), everyAgent);
  return subtree ? { sheet: "subtree", subtree } : { sheet: "stop_work", stopWork: stopWorkOf(panes, hostAgents) };
}

/** The close sheet's title (D-17): how many descendants close with it. */
export function subtreeTitle(kind: CloseKind, count: number): string {
  return kind === "pane" ? `이 에이전트와 자식 ${count}개를 닫을까요?` : `이 탭과 자식 ${count}개를 닫을까요?`;
}

/** The phases in which a close the core runs still names its pane. */
const CLOSING_PHASES = new Set(["waiting", "closing", "transmitting", "awaiting_topology", "unknown"]);

/**
 * Whether an agent's pane is being closed, on its own or as a node of a
 * tree close (B7): the sidebar row says `closing…` until the row goes.
 */
export function agentClosing(operations: readonly AsyncOperation[] | undefined, paneId: string): boolean {
  return (operations ?? []).some((op) => (op.kind === "tree.close" || op.kind === "pane.close") && op.target_id === paneId && CLOSING_PHASES.has(op.phase));
}
