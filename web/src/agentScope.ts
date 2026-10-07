import type { AgentRow, DescendantCounts, MarkCounts, RequestVerb, SnapshotRest } from "./snapshot";

/** Core-owned membership, status counts and display order for one scope. */
export type AgentScope = {
  pane_ids: string[];
  total: number;
  overview_total: number;
  roots: string[];
  groups: { needs_you: number; done: number; working: number; seen: number };
  marks: MarkCounts;
  group_rows: { group: string; pane_ids: string[] }[];
  descendants: Record<string, number>;
  children: Record<string, string[]>;
  sections: { group: string; rows: { pane_id: string; depth: number; descendants: number }[]; count: number }[];
  members: { pane_id: string; project_id: string; checkout_id: string }[];
  buckets: { turn: number; working: number; delegating: number; resting: number };
  turns: { question: number; approval: number; error: number; done: number };
  folded: Record<string, {
    tiers: { key: string; candidates: string[]; branch: string | null; pull_request: number | null; device: string | null }[][];
    overflow: number;
    badge_descendants: number;
    badge_counts: DescendantCounts;
    badge_children: string[];
  }>;
  tree: AgentTreeScope;
  global_tree: AgentTreeScope;
  requests: {
    rows: { member: number; children: string[] }[];
    groups: { verb: RequestVerb; rows: number[] }[];
    counts: Record<RequestVerb, number>;
    todo: number;
    answer: number;
  };
};

/** No snapshot yet means no known scope; a received scope is never rebuilt here. */
export function deviceScope(rest: SnapshotRest | null, id: string): AgentScope | null {
  return rest?.navigator?.devices?.find((device) => device.id === id)?.agent_scope ?? null;
}

export function scopeRows(ids: readonly string[], agents: readonly AgentRow[]): AgentRow[] {
  const byPane = new Map(agents.map((agent) => [agent.pane_id, agent]));
  return ids.map((id) => {
    const row = byPane.get(id);
    if (!row) throw new Error(`Agent scope references a missing row: ${id}`);
    return row;
  });
}

export type AgentTreeScope = {
  rows: { pane_id: string; depth: number }[];
  visible_rows: { pane_id: string; depth: number }[];
  shown: string[];
  more: number;
  needs_you: boolean;
};
