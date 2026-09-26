// What an agent row shows, as pure rules over the core's row (PRD
// sidebar-agent-status D-05, B7-B9). The core decides the group, the mark, the
// read axis, the waiting state and the sentence; these rules decide only when
// a view draws the sentence and how the mark and badge are coloured, so the
// sidebar and any other list of agents (the Overview) cannot disagree.

import { chipTone } from "./lineage";
import type { AgentRow, DescendantCounts } from "./snapshot";

/**
 * Why a row's second line is on screen at rest.
 *
 * - `request`: the agent is asking the operator (question, approval, error);
 *   the line stays until the request is resolved, reading it does not end it.
 * - `news`: the row changed since the operator last looked; it goes once read.
 * - `quiet`: nothing to report; the line shows only on the selected or hovered
 *   row, where it is revealed in full.
 */
export type LineMode = "request" | "news" | "quiet";

export type RowLine = { text: string; mode: LineMode };

const REQUESTS = new Set(["question", "approval", "error"]);

/** The row's second line and why it shows, or null when the core gave it no sentence. */
export function rowLine(agent: Pick<AgentRow, "detail" | "demand" | "unread">): RowLine | null {
  const text = agent.detail?.trim();
  if (!text) return null;
  if (REQUESTS.has(agent.demand ?? "none")) return { text, mode: "request" };
  if (agent.unread) return { text, mode: "news" };
  return { text, mode: "quiet" };
}

/**
 * The line a sidebar row draws (PRD sidebar-readability D-4, B4, B6): a
 * request or news only, on one line from the moment it exists, so a pointer,
 * the keyboard or a selection never adds a line or grows the row. A quiet
 * sentence is read in the row's tooltip.
 */
export function sidebarLine(agent: Pick<AgentRow, "detail" | "demand" | "unread">): RowLine | null {
  const line = rowLine(agent);
  return line && line.mode !== "quiet" ? line : null;
}

/** Whether the line is drawn without a pointer or selection on the row. */
export function lineShownAtRest(line: RowLine, selected: boolean): boolean {
  return line.mode !== "quiet" || selected;
}

/** The colour of a line: a request in its demand's colour, news bright, a revealed line subdued. */
export function lineTone(line: RowLine, demand: string | undefined): string {
  if (line.mode === "request") return demand === "error" ? "text-destructive" : "text-warning";
  if (line.mode === "news") return "text-foreground";
  return "text-subtle-foreground";
}

/**
 * The status mark's colour. A root waiting on its children draws its hollow
 * ring in the working colour (D-01); every other row reads its own axes the
 * way the chips do.
 */
export function markTone(agent: Pick<AgentRow, "demand" | "activity" | "emphasized" | "waiting_on_descendants">): string {
  if (agent.waiting_on_descendants) return "text-agent-working";
  return chipTone({ demand: agent.demand ?? "none", activity: agent.activity ?? "", emphasized: agent.emphasized });
}

/** One mark and count on the descendant badge. */
export type BadgePart = { state: keyof DescendantCounts; symbol: string; count: number; tone: string };

const BADGE_ORDER: { state: keyof DescendantCounts; symbol: string; tone: string }[] = [
  { state: "error", symbol: "×", tone: "text-destructive" },
  { state: "approval", symbol: "!", tone: "text-warning" },
  { state: "question", symbol: "?", tone: "text-warning" },
  { state: "working", symbol: "●", tone: "text-agent-working" },
  { state: "done", symbol: "✓", tone: "text-success" },
];

/**
 * The badge's marks, worst first, with zero states left out (docs/status-model.md).
 * A ready descendant and one Herdr cannot classify are counted in none.
 */
export function badgeParts(counts: DescendantCounts | undefined): BadgePart[] {
  if (!counts) return [];
  return BADGE_ORDER.filter(({ state }) => counts[state] > 0).map((part) => ({ ...part, count: counts[part.state] }));
}

/** The badge's accessible name: how many live descendants and what they are doing. */
export function badgeLabel(counts: DescendantCounts | undefined, live: number): string {
  const parts = badgeParts(counts).map((part) => `${part.count} ${part.state}`);
  return `${live} live ${live === 1 ? "descendant" : "descendants"}${parts.length > 0 ? `: ${parts.join(", ")}` : ""}`;
}

/**
 * The branch chip: only a delegated row whose checkout differs from its
 * parent's carries one (B9). The core decides the difference and names the
 * checkout; a root's context in a group list has no checkout to differ from.
 */
export function branchChip(agent: Pick<AgentRow, "delegated" | "lineage_worktree_badge">): string | null {
  if (!agent.delegated) return null;
  const badge = agent.lineage_worktree_badge?.trim();
  return badge ? badge : null;
}

/**
 * The row's accessible name: title, the device and branch chips the row
 * shows, kind, status word, then the sentence (docs/status-model.md). The
 * row's visible text is hidden from assistive technology, so everything it
 * shows has to be here.
 */
export function rowAccessibleName(agent: AgentRow, device: string | null): string {
  return [agent.identity_label, device, branchChip(agent), agent.agent_kind, agent.status_label, agent.detail].filter(Boolean).join(", ");
}

/** A row in a drawn agent tree: the row, its device, and how deep it sits under the root drawn above it. */
export type TreeRow = { agent: AgentRow; device: string | null; depth: number; descendants: number };

/**
 * The rows one group section draws: its roots in the core's order, each
 * followed by its descendants in lineage order while the operator has it
 * unfolded (`lineage_collapsed` false). A delegated row is drawn under its
 * parent and never on its own, so a folded parent speaks for its children
 * through its badge. `byPane` indexes the device's own rows, since pane ids
 * are scoped to a device.
 */
export function sectionTree(
  roots: { agent: AgentRow; device: string | null }[],
  byPane: (device: string | null, paneId: string) => AgentRow | undefined,
  descendantsOf: (device: string | null, paneId: string) => number,
): TreeRow[] {
  const rows: TreeRow[] = [];
  const visit = (agent: AgentRow, device: string | null, depth: number, seen: Set<string>) => {
    if (seen.has(agent.pane_id)) return;
    seen.add(agent.pane_id);
    rows.push({ agent, device, depth, descendants: descendantsOf(device, agent.pane_id) });
    if (agent.lineage_collapsed !== false) return;
    for (const id of agent.lineage_child_pane_ids ?? []) {
      const child = byPane(device, id);
      if (child) visit(child, device, depth + 1, seen);
    }
  };
  for (const { agent, device } of roots) {
    if (agent.delegated) continue;
    visit(agent, device, 0, new Set());
  }
  return rows;
}

/**
 * How many agents a group heading speaks for: each root it lists and every
 * live descendant beneath it, folded or not, so a delegated row counts once,
 * under the heading its parent is drawn in.
 */
export function sectionCount(rows: TreeRow[]): number {
  return rows.filter((row) => row.depth === 0).reduce((total, row) => total + 1 + row.descendants, 0);
}

/** The direct children the badge's popover lists, in lineage order, that are still rows. */
export function directChildren(agent: AgentRow, byPane: (paneId: string) => AgentRow | undefined): AgentRow[] {
  return (agent.lineage_child_pane_ids ?? []).map(byPane).filter((row): row is AgentRow => row !== undefined);
}

/**
 * The rows of a lineage drawn root first (`checkoutAgentRows`), less the
 * descendants of every parent the operator has folded (`lineage_collapsed`
 * not false), the same core choice the Agents list folds by (PRD
 * sidebar-readability D-6, B12). A folded parent's badge speaks for what is
 * left out.
 */
export function unfoldedRows<Row extends { agent: Pick<AgentRow, "lineage_collapsed">; depth: number }>(rows: Row[]): Row[] {
  const drawn: Row[] = [];
  let foldedAt: number | null = null;
  for (const row of rows) {
    if (foldedAt !== null && row.depth > foldedAt) continue;
    foldedAt = row.agent.lineage_collapsed === false ? null : row.depth;
    drawn.push(row);
  }
  return drawn;
}
