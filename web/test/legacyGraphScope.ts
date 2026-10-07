// Frozen graph status and fold rules from main 9f144877.
// Geometry assertions continue to use the production renderer.
import type { AgentGraphScope, AgentScope } from "../src/agentScope";
import type { AgentRow, Workspace } from "../src/snapshot";

export function legacyGraphScope(project: Workspace, members: AgentScope["members"], agents: readonly AgentRow[]): AgentGraphScope {
  const byPane = new Map(agents.map((a) => [a.pane_id, a]));
  const primary = project.checkouts.find((c) => c.is_primary === true) ?? (project.is_git ? null : project.checkouts[0]);
  const value: AgentGraphScope = { attention: 4, recency: "", checkouts: {}, variants: [], tucked: [{}] };
  for (const checkout of project.checkouts) {
    const own = members.filter((m) => m.checkout_id === checkout.id).map((m) => byPane.get(m.pane_id)!);
    own.sort((a, b) => a.state.graph_rank - b.state.graph_rank || (b.last_activity ?? "").localeCompare(a.last_activity ?? ""));
    const rank = own[0]?.state.graph_rank ?? 4;
    const resting = own.every((a) => a.state.graph_rank === 3);
    const cleanup = !checkout.is_worktree || checkout.is_primary === true || project.is_git !== true ? null : !checkout.exists || checkout.worktree?.missing === true ? "missing" : checkout.landed || checkout.pull_request?.badge === "merged" ? "merged" : null;
    value.checkouts[checkout.id] = { primary: primary?.id === checkout.id, cleanup, fold: cleanup && resting ? "cleanup" : own.length === 0 ? "empty" : resting ? "resting" : null, members: own.map((a) => a.pane_id), rank, resting };
    value.attention = Math.min(value.attention, rank) as AgentGraphScope["attention"];
    for (const agent of own) if ((agent.last_activity ?? "") > value.recency) value.recency = agent.last_activity!;
  }
  const owner = new Map(members.map((m) => [m.pane_id, m.checkout_id]));
  const marks: Record<string, "error" | "approval" | "question" | "working" | "done" | "idle"> = { "×": "error", "!": "approval", "?": "question", "●": "working", "✓": "done", "○": "idle" };
  for (let selector = 0; selector < 16; selector++) {
    const shown = new Set(Object.entries(value.checkouts).filter(([, c]) => (c.primary && (selector & 8)) || c.fold === null || (selector & { empty: 1, cleanup: 2, resting: 4 }[c.fold])).map(([id]) => id));
    const badges: AgentGraphScope["tucked"][number] = {};
    for (const member of members) {
      if (shown.has(member.checkout_id)) continue;
      const agent = byPane.get(member.pane_id)!;
      let ancestor = agent.lineage_parent_pane_id ? byPane.get(agent.lineage_parent_pane_id) : undefined;
      const seen = new Set<string>();
      while (ancestor && owner.has(ancestor.pane_id) && !shown.has(owner.get(ancestor.pane_id)!) && !seen.has(ancestor.pane_id)) {
        seen.add(ancestor.pane_id);
        ancestor = ancestor.lineage_parent_pane_id ? byPane.get(ancestor.lineage_parent_pane_id) : undefined;
      }
      const mark = marks[agent.symbol];
      if (ancestor && shown.has(owner.get(ancestor.pane_id)!) && mark) {
        const counts = badges[ancestor.pane_id] ??= {};
        counts[mark] = (counts[mark] ?? 0) + 1;
      }
    }
    const index = value.tucked.findIndex((b) => JSON.stringify(b) === JSON.stringify(badges));
    value.variants.push(index < 0 ? value.tucked.push(badges) - 1 : index);
  }
  return value;
}
