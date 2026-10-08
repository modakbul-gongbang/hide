// Frozen checkout tree fixture rules from main 9f144877.
import type { AgentRow, Checkout, Workspace } from "../src/snapshot";
import type { BoardRow } from "../src/projectBoard";
export function checkoutAgentRows(workspace: Workspace, agents: AgentRow[]): Map<string, BoardRow[]> {
  const owners = new Map<string, Checkout>();
  for (const checkout of workspace.checkouts) {
    for (const tab of checkout.tabs) for (const pane of tab.panes) if (!owners.has(pane.id)) owners.set(pane.id, checkout);
  }
  const byPane = new Map<string, AgentRow>();
  for (const agent of agents) if (!byPane.has(agent.pane_id)) byPane.set(agent.pane_id, agent);
  const treeRows = (roots: AgentRow[]): BoardRow[] => {
    const rows: BoardRow[] = [];
    const seen = new Set<string>();
    const append = (agent: AgentRow, depth: number) => {
      if (seen.has(agent.pane_id)) return;
      seen.add(agent.pane_id);
      rows.push({ agent, depth });
      for (const childId of agent.lineage_child_pane_ids ?? []) {
        const child = byPane.get(childId);
        if (child) append(child, depth + 1);
      }
    };
    for (const root of roots) append(root, 0);
    return rows;
  };
  const checkoutRows = (checkout: Checkout): BoardRow[] => {
    const local = agents.filter((agent) => owners.get(agent.pane_id)?.id === checkout.id);
    const localIds = new Set(local.map((agent) => agent.pane_id));
    return treeRows(local.filter((agent) => !agent.lineage_parent_pane_id || !localIds.has(agent.lineage_parent_pane_id)));
  };
  return new Map(workspace.checkouts.map((checkout) => [checkout.id, checkoutRows(checkout)]));
}
