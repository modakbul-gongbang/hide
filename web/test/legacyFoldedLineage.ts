// Frozen pre-refactor lineage fixture oracle, main 9f144877.
import type { AgentRow, Checkout, DescendantCounts, Workspace } from "../src/snapshot";

export type CheckoutLine = {
  key: string;
  symbol: string;
  agent: AgentRow;
  /** The checkout's branch or label; null when neither it nor the agent names one. */
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

type CheckoutFact = {
  key: string;
  checkout: Checkout;
  deviceId: string;
};

function attention(agent: AgentRow): number {
  return agent.state.attention_rank;
}

function mark(counts: DescendantCounts, agent: AgentRow) {
  if (agent.demand === "error") counts.error += 1;
  else if (agent.demand === "approval") counts.approval += 1;
  else if (agent.demand === "question") counts.question += 1;
  else if (agent.activity === "working") counts.working += 1;
  else if (agent.group === "done") counts.done += 1;
}

function checkoutFacts(workspaces: Workspace[]): Map<string, CheckoutFact> {
  const facts = new Map<string, CheckoutFact>();
  for (const workspace of workspaces) {
    for (const checkout of workspace.checkouts) {
      const fact = { key: `${workspace.device_id}\u0000${checkout.id}`, checkout, deviceId: workspace.device_id };
      for (const tab of checkout.tabs) {
        for (const pane of tab.panes) facts.set(pane.id, fact);
      }
    }
  }
  return facts;
}

/**
 * The folded root's derived presentation. Other-checkout descendants become
 * C lines; only descendants in the root's checkout remain in its badge.
 */
export function foldedLineage(parent: AgentRow, agents: AgentRow[], workspaces: Workspace[]): FoldedLineage {
  const byPane = new Map(agents.map((agent) => [agent.pane_id, agent]));
  const facts = checkoutFacts(workspaces);
  const parentFact = facts.get(parent.pane_id);
  const descendants: AgentRow[] = [];
  const seen = new Set<string>();
  const queue = [...(parent.lineage_child_pane_ids ?? [])];
  while (queue.length > 0) {
    const paneId = queue.shift()!;
    if (seen.has(paneId) || paneId === parent.pane_id) continue;
    const agent = byPane.get(paneId);
    if (!agent) continue;
    seen.add(paneId);
    descendants.push(agent);
    queue.push(...(agent.lineage_child_pane_ids ?? []));
  }

  const sameCheckout = (agent: AgentRow) => {
    const fact = facts.get(agent.pane_id);
    return parentFact ? fact?.key === parentFact.key : agent.checkout_label === parent.checkout_label;
  };
  const badgeRows = descendants.filter(sameCheckout);
  const badgeCounts: DescendantCounts = { error: 0, approval: 0, question: 0, working: 0, done: 0 };
  for (const agent of badgeRows) mark(badgeCounts, agent);
  const badgeChildren = (parent.lineage_child_pane_ids ?? [])
    .map((paneId) => byPane.get(paneId))
    .filter((agent): agent is AgentRow => agent !== undefined && sameCheckout(agent));

  const grouped = new Map<string, { fact: CheckoutFact | null; rows: AgentRow[] }>();
  for (const agent of descendants) {
    if (sameCheckout(agent)) continue;
    const fact = facts.get(agent.pane_id) ?? null;
    const key = fact?.key ?? `${agent.device_id ?? "unknown"}\u0000${agent.checkout_label ?? agent.pane_id}`;
    const group = grouped.get(key) ?? { fact, rows: [] };
    group.rows.push(agent);
    grouped.set(key, group);
  }
  const lines = [...grouped.entries()]
    .map(([key, group]) => {
      const agent = group.rows.slice().sort((a, b) => attention(a) - attention(b) || a.identity_label.localeCompare(b.identity_label))[0]!;
      const branch = group.fact?.checkout.branch ?? group.fact?.checkout.label ?? agent.checkout_label ?? null;
      return {
        key,
        symbol: agent.symbol,
        agent,
        branch,
        pullRequest: group.fact?.checkout.pull_request?.number ?? null,
        device: agent.device_id !== parent.device_id ? (agent.device_label ?? null) : null,
      };
    })
    .sort((a, b) => attention(a.agent) - attention(b.agent) || (a.branch ?? "").localeCompare(b.branch ?? ""));

  return {
    lines: lines.slice(0, 3),
    overflow: Math.max(0, lines.length - 3),
    badgeDescendants: badgeRows.length,
    badgeCounts,
    badgeChildren,
  };
}
