// Frozen row-tree rules from main 9f144877.
import type { AgentRow } from "../src/snapshot";
import type { TreeRow } from "../src/agentRow";
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
