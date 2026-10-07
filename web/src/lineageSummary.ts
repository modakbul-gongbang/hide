import { scopeRows, type AgentScope } from "./agentScope";
import type { AgentRow, DescendantCounts } from "./snapshot";

export type CheckoutLine = {
  key: string;
  symbol: string;
  agent: AgentRow;
  branch: string | null;
  pullRequest: number | null;
  device: string | null;
};
export type FoldedLineage = {
  lines: CheckoutLine[];
  overflow: number;
  badgeDescendants: number;
  badgeCounts: DescendantCounts;
  badgeChildren: AgentRow[];
};

/** Core picks membership and status priority; the browser keeps its existing
 * locale-aware alphabetical placement within an equal-priority tier. */
export function foldedLineage(parent: AgentRow, agents: AgentRow[], scope: AgentScope): FoldedLineage {
  const value = scope.folded[parent.pane_id];
  if (!value) throw new Error(`Missing folded lineage for ${parent.pane_id}`);
  const lines = value.tiers.flatMap((tier) => tier.map((line) => {
    const agent = scopeRows(line.candidates, agents).sort((a, b) => a.identity_label.localeCompare(b.identity_label))[0];
    if (!agent) throw new Error("A lineage line must have a representative");
    return { key: line.key, agent, symbol: agent.symbol, branch: line.branch, pullRequest: line.pull_request, device: line.device };
  }).sort((a, b) => (a.branch ?? "").localeCompare(b.branch ?? "")));
  return { lines: lines.slice(0, 3), overflow: value.overflow, badgeDescendants: value.badge_descendants,
    badgeCounts: value.badge_counts, badgeChildren: scopeRows(value.badge_children, agents) };
}
