import type { AgentRow, DescendantCounts, MarkCounts, RequestVerb, SnapshotRest } from "./snapshot";

/** Core-owned membership, status counts and display order for one scope. */
export type AgentScope = {
  closes: Record<string, CloseScope>;
  raised: { group: "needs_you" | "done"; shown: string[]; more: string[] }[];
  owners: Record<string, string>;
  badge_total: number;
  prs: {
    rows: { number: number; checkout_id: string | null; agents: string[]; lineage: { pane_id: string; depth: number }[]; needs_look: boolean; group: "turn" | "fixing" | "blocked" | "merged"; issue: { key: string; label: string; url: string | null; task_key: string | null } | null }[];
    groups: { group: "turn" | "fixing" | "blocked" | "merged"; numbers: number[] }[];
    open: number;
  };
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
  turn_kind: "question" | "review" | null;
};

export type CloseScope = {
  decision: { action: "status_unknown"; label: string } | { action: "confirm" } | { action: "close" };
  stop_work: { rows: { pane_id: string; agent: boolean; label: string; state: "active" | "unknown" | "quiet" }[]; unknown: number | null };
  subtree: CloseSubtree | null;
  subtree_all: CloseSubtree | null;
};
export type CloseSubtree = {
  ids: string[];
  rows: { pane_id: string; depth: number; target: boolean; state: "working" | "waiting" | "unread" | "unknown" | "quiet" }[];
  counts: { working: number; waiting: number; unread: number; unknown: number };
  unknown: boolean;
  target_unknown: boolean;
};
