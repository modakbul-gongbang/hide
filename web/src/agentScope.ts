import type { AgentRow, DescendantCounts, MarkCounts, RequestVerb, SessionGroup, SnapshotRest } from "./snapshot";

/** Core-owned membership, status counts and display order for one scope. */
export type AgentRef = { pane_id: string; occurrence: number };

export type RowWork = { pull: number | null; more: number; issues: string[]; issue_chips: string[] };

export type AgentScope = {
  sessions: {
    counts: Record<SessionGroup, number>;
    groups: { group: SessionGroup; members: number[] }[];
  };
  overview_needs_you: number;
  work: Record<string, RowWork>;
  has_working: boolean;
  relations: Record<string, { issues: string[]; project_id: string; checkout_id: string; rows: { pane_id: string; occurrence: number; depth: number; tag: "here" | "parent" | null; caption_parent: string | null }[] }[]>;
  listed: { pane_id: string; device_id: string; device_label: string | null; remote: boolean; index: number }[];
  places: Record<string, { project_id: string; checkout_id: string; kind: "home" | "folder" | "checkout" }>;
  places_live: boolean;
  graph: AgentGraphScope;
  closes: Record<string, CloseScope>;
  raised: { group: "needs_you" | "done"; shown: AgentRef[]; more: AgentRef[] }[];
  owners: Record<string, string>;
  badge_total: number;
  prs: {
    counts: Record<"turn" | "fixing" | "blocked" | "review" | "draft" | "look", number>;
    rows: { number: number; checkout_id: string | null; agents: AgentRef[]; lineage: (AgentRef & { depth: number })[]; needs_look: boolean; group: "turn" | "fixing" | "blocked" | "merged"; issue: { key: string; label: string; url: string | null; task_key: string | null } | null }[];
    groups: { group: "turn" | "fixing" | "blocked" | "merged"; numbers: number[] }[];
    open: number;
  };
  rows: AgentRef[];
  total: number;
  overview_total: number;
  roots: AgentRef[];
  groups: { needs_you: number; done: number; working: number; seen: number };
  marks: MarkCounts;
  group_rows: { group: string; rows: AgentRef[] }[];
  descendants: Record<string, number>;
  children: Record<string, string[]>;
  sections: { group: string; rows: (AgentRef & { depth: number; descendants: number })[]; count: number }[];
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

/** Resolve physical occurrences without changing their core-selected membership. */
export function scopeOccurrences(refs: readonly AgentRef[], agents: readonly AgentRow[]): AgentRow[] {
  const byPane = new Map<string, AgentRow[]>();
  for (const row of agents) { const found = byPane.get(row.pane_id) ?? []; found.push(row); byPane.set(row.pane_id, found); }
  return refs.map((ref) => {
    const row = byPane.get(ref.pane_id)?.[ref.occurrence];
    if (!row) throw new Error(`Missing agent occurrence: ${ref.pane_id}/${ref.occurrence}`);
    return row;
  });
}

export type AgentTreeScope = {
  rows: (AgentRef & { depth: number })[];
  visible_rows: (AgentRef & { depth: number })[];
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

export type AgentGraphCross = {
  direction: "out" | "in";
  project_id: string;
  project_device_id: string;
  /** Null means the same device; a null label names this machine. */
  device: { label: string | null } | null;
  pane_ids: string[];
  names: string[];
  box_id: string;
  count: number;
};
export type AgentGraphScope = {
  cross: Record<string, AgentGraphCross[]>;
  attention: 0 | 1 | 2 | 3 | 4;
  recency: string;
  checkouts: Record<string, { primary: boolean; cleanup: "merged" | "missing" | null; fold: "empty" | "cleanup" | "resting" | null; members: string[]; rank: 0 | 1 | 2 | 3 | 4; resting: boolean }>;
  variants: number[];
  tucked: Record<string, Partial<Record<"error" | "approval" | "question" | "working" | "done" | "idle", number>>>[];
};
