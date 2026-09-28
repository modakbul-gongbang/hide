// The Overview's lenses (PRD overview-lenses-tiles-agents) as pure functions
// over the snapshot: the tiles that stand where the tab row was, and the two
// modes of the Agents tab, checkout lanes and lineage. Every value comes from
// the snapshot and a value not read yet is null, never a zero (design 10).
// The inbox these replace grouped agents by what they wanted; here the lane
// order, the node order and the yellow nodes carry that instead (D-03).

import type { DeviceAvailability } from "./navigation";
import { stageOf, type BoardProject, type TasksBoard } from "./projectBoard";
import type { AgentRow, Checkout, ProjectSessions, Task, Workspace } from "./snapshot";
import { primaryCheckout } from "./workspaceManage";

// --- agents ------------------------------------------------------------------

/**
 * Where an agent stands for the operator (D-37, D-38): its turn (asking, or
 * finished and not looked at), working, waiting on its children, or resting.
 * Lanes, nodes, lineages and the Agents tile all read this one order.
 */
export type AgentBucket = "turn" | "working" | "delegating" | "resting";

const BUCKETS: readonly { bucket: AgentBucket; label: string }[] = [
  { bucket: "turn", label: "내 차례" },
  { bucket: "working", label: "일하는 중" },
  { bucket: "delegating", label: "자식 대기" },
  { bucket: "resting", label: "쉬는 중" },
];

const BUCKET_RANK: Record<AgentBucket, number> = { turn: 0, working: 1, delegating: 2, resting: 3 };

export function bucketOf(agent: AgentRow): AgentBucket {
  if (agent.group === "needs_you" || agent.group === "done") return "turn";
  if (agent.waiting_on_descendants) return "delegating";
  if (agent.group === "working") return "working";
  return "resting";
}

/** The later of two activity keys; the core pads them so they sort as text. */
function later(a: string, b: string): string {
  return a > b ? a : b;
}

/** The most recent activity among `nodes`. */
function latestActivity(nodes: readonly LensAgent[]): string {
  return nodes.reduce((best, node) => later(best, node.agent.last_activity ?? ""), "");
}

/** A lane's or a lineage's place: the operator's turn if any agent's, else working if any works or waits on children, else resting. */
function rankOf(nodes: readonly LensAgent[]): "turn" | "working" | "resting" {
  const buckets = new Set(nodes.map((node) => node.bucket));
  return buckets.has("turn") ? "turn" : buckets.has("working") || buckets.has("delegating") ? "working" : "resting";
}

/** Turn first, then working, waiting on children, resting; the most recently active first inside one. */
function byBucketThenActivity(a: AgentRow, b: AgentRow): number {
  return BUCKET_RANK[bucketOf(a)] - BUCKET_RANK[bucketOf(b)] || (b.last_activity ?? "").localeCompare(a.last_activity ?? "");
}

/** One agent the Overview draws, with where it works. */
export type LensAgent = {
  agent: AgentRow;
  bucket: AgentBucket;
  project: Workspace;
  checkout: Checkout;
  /** The SSH device it runs on, or null for this machine. */
  device: string | null;
  /** The issue its checkout works on. */
  task: Task | null;
};

/** Every agent whose pane one of the scope's checkouts holds, in the core's order. */
export function scopeAgents(projects: readonly BoardProject[]): LensAgent[] {
  const result: LensAgent[] = [];
  for (const { workspace, agents, device } of projects) {
    const owners = new Map<string, Checkout>();
    for (const checkout of workspace.checkouts) for (const tab of checkout.tabs) for (const pane of tab.panes) if (!owners.has(pane.id)) owners.set(pane.id, checkout);
    const tasks = new Map((workspace.tasks?.tasks ?? []).map((task) => [task.key, task]));
    const seen = new Set<string>();
    for (const agent of agents) {
      const checkout = owners.get(agent.pane_id);
      if (!checkout || seen.has(agent.pane_id)) continue;
      seen.add(agent.pane_id);
      result.push({ agent, bucket: bucketOf(agent), project: workspace, checkout, device, task: checkout.task_key ? (tasks.get(checkout.task_key) ?? null) : null });
    }
  }
  return result;
}

// --- tiles -------------------------------------------------------------------

/** One stretch of a tile's bar, with the name and count its legend reads. */
export type TileSegment = { key: string; label: string; count: number };

export type TileId = "agents" | "issues" | "sessions";

/**
 * A lens tile (D-02): its name, the big number and its unit, the yellow
 * badge of what waits on the operator with its breakdown, and one bar.
 * `value` null is a value not read yet (the number place stays empty and
 * there is no bar); `failure` is a read that failed, drawn as ⚠ by the name.
 */
export type Tile = {
  id: TileId;
  label: string;
  value: number | null;
  unit: string | null;
  badge: { count: number; parts: TileSegment[] } | null;
  bar: TileSegment[] | null;
  failure: string | null;
};

/**
 * The Agents tile (B3): how many agents the scope has, the badge of those
 * whose turn it is (asking, or finished and not looked at) split into
 * questions, approvals, errors and finished ones, and the bar of the four
 * buckets. A device that has not answered has no count.
 */
export function agentsTile(agents: readonly LensAgent[], availability: DeviceAvailability): Tile {
  const known = availability.state === "ready";
  const count = (bucket: AgentBucket) => agents.filter((value) => value.bucket === bucket).length;
  const demand = (kind: string) => agents.filter((value) => value.agent.group === "needs_you" && value.agent.demand === kind).length;
  const turn = count("turn");
  const parts = [
    { key: "question", label: "질문", count: demand("question") },
    { key: "approval", label: "승인", count: demand("approval") },
    { key: "error", label: "오류", count: demand("error") },
    { key: "done", label: "끝남", count: agents.filter((value) => value.agent.group === "done").length },
  ].filter((part) => part.count > 0);
  return {
    id: "agents",
    label: "Agents",
    value: known ? agents.length : null,
    unit: null,
    badge: known && turn > 0 ? { count: turn, parts } : null,
    bar: known ? BUCKETS.map(({ bucket, label }) => ({ key: bucket, label, count: count(bucket) })) : null,
    // The agents are the device's live rows, so a device that cannot answer
    // has no last value to keep: the ⚠ says why (B6), and the count stays empty.
    failure: availability.state === "unavailable" ? `에이전트를 읽지 못함 · ${availability.text}` : null,
  };
}

/**
 * The Issues tile (B3): the open issues once the source has answered, and
 * the bar of where they stand, the backlog, in progress and in review. A
 * failed read keeps the last value and says so by the name (B6).
 */
export function issuesTile(board: TasksBoard, now: number, lastReadAt: number | null): Tile {
  const open = board.source.openIssues;
  const stage = (value: "backlog" | "working" | "review") => board.cards.filter((card) => card.task?.open && card.stage === value).length;
  const failure = board.source.failure ? `${board.source.failure}${lastReadAt === null ? "" : ` · ${ageWords(now - lastReadAt)} 값`}` : null;
  return {
    id: "issues",
    label: "Issues",
    value: open,
    unit: "열림",
    badge: null,
    bar:
      open === null
        ? null
        : [
            { key: "backlog", label: "백로그", count: stage("backlog") },
            { key: "working", label: "진행 중", count: stage("working") },
            { key: "review", label: "리뷰", count: stage("review") },
          ],
    failure,
  };
}

/** When the project's issue source was last read, for a failure's age. */
export function lastIssueRead(workspace: Workspace): number | null {
  return workspace.tasks?.source?.last_read_at_unix_ms ?? null;
}

/** The start of the local day `now` falls in: "today" is this machine's date (B5). */
export function startOfDay(now: number): number {
  const date = new Date(now);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

/**
 * The Sessions tile (D-16, B5): the sessions of this Project active today
 * (last updated since local midnight), split Claude and Codex. Empty until
 * the history named for this Project has answered; a failed read keeps what
 * it had and says so by the name.
 */
export function sessionsTile(sessions: ProjectSessions | null | undefined, workspaceId: string, now: number): Tile {
  const mine = sessions && sessions.workspace_id === workspaceId ? sessions : null;
  const failure = mine ? (mine.failure ?? mine.unavailable_reason) : null;
  const answered = mine !== null && !(mine.loading && mine.rows.length === 0) && !(failure && mine.rows.length === 0);
  const since = startOfDay(now);
  const today = answered ? mine.rows.filter((row) => row.updated_at_unix_ms >= since) : [];
  return {
    id: "sessions",
    label: "Sessions",
    value: answered ? today.length : null,
    unit: "오늘",
    badge: null,
    bar: answered
      ? [
          { key: "claude", label: "Claude", count: today.filter((row) => row.provider === "claude").length },
          { key: "codex", label: "Codex", count: today.filter((row) => row.provider === "codex").length },
        ]
      : null,
    failure: failure ? `세션 기록을 읽지 못함 · ${mine?.rows.length ? "마지막으로 읽은 값" : "읽은 값 없음"}` : null,
  };
}

/** `3분 전`, `2시간 전`, the age a failure's popover gives its last value. */
export function ageWords(ms: number): string {
  const minutes = Math.max(0, Math.floor(ms / 60_000));
  if (minutes < 1) return "방금";
  if (minutes < 60) return `${minutes}분 전`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}시간 전`;
  return `${Math.floor(hours / 24)}일 전`;
}

// --- checkout lanes --------------------------------------------------------

/** Why a worktree is only there to be removed: its work is merged, or its folder is gone. */
export type Cleanup = "merged" | "missing";

function cleanupOf(workspace: Workspace, checkout: Checkout): Cleanup | null {
  if (!checkout.is_worktree || checkout.is_primary === true || workspace.is_git !== true) return null;
  if (!checkout.exists || checkout.worktree?.missing === true) return "missing";
  return stageOf(checkout) === "done" ? "merged" : null;
}

/** One agent in a lane, at the column that keeps its delegation line straight. */
export type LaneNode = LensAgent & { column: number };

/** A lane's place in the order (B13): the primary checkout, then whose turn, working, resting. */
export type LaneRank = "primary" | "turn" | "working" | "resting";

/** One checkout's lane (D-05): its head facts come from `checkout`, its agents are `nodes`. */
export type Lane = {
  id: string;
  project: Workspace;
  checkout: Checkout;
  primary: boolean;
  rank: LaneRank;
  nodes: LaneNode[];
  cleanup: Cleanup | null;
  task: Task | null;
};

/** A delegation line: from the parent's node to the child's, down across lanes or right within one (B14). */
export type Delegation = { from: string; to: string; within: boolean };

export type LanesBoard = {
  lanes: Lane[];
  /** Worktrees with no agent, folded into one line (B20). */
  empty: Lane[];
  /** Merged or folder-less worktrees whose agents all rest, folded into one line (B20). */
  cleanup: Lane[];
  delegations: Delegation[];
  /** How many node columns the widest lane needs. */
  columns: number;
};

const RANK_ORDER: Record<LaneRank, number> = { primary: 0, turn: 1, working: 2, resting: 3 };

/**
 * The checkout lanes (D-05, D-38, B13, B14, B20). A lane is a checkout and
 * its agents; the primary checkout stands first, then lanes with an agent
 * whose turn it is, working lanes, resting lanes, the most recently active
 * first inside each. On All projects every project's primary checkout is
 * ranked by its agents like any lane, first among equals, so one project's
 * resting main never stands above another's turn; an idle one folds.
 * Inside a lane the nodes go turn, working, waiting on children, resting.
 * A node delegated from a node in an earlier lane takes its parent's column
 * when that column is still free, so the line runs straight down; nothing
 * else leaves a gap. Worktrees with no agent, and merged or folder-less ones
 * whose agents all rest, fold into two lines.
 */
export function buildLanes(projects: readonly BoardProject[], agents: readonly LensAgent[], scope: "project" | "all"): LanesBoard {
  const byCheckout = new Map<string, LensAgent[]>();
  for (const value of agents) byCheckout.set(value.checkout.id, [...(byCheckout.get(value.checkout.id) ?? []), value]);
  const lanes: Lane[] = [];
  const empty: Lane[] = [];
  const cleanup: Lane[] = [];
  for (const { workspace } of projects) {
    const primaryId = primaryCheckout(workspace)?.id ?? null;
    const tasks = new Map((workspace.tasks?.tasks ?? []).map((task) => [task.key, task]));
    for (const checkout of workspace.checkouts) {
      const members = (byCheckout.get(checkout.id) ?? []).slice().sort((a, b) => byBucketThenActivity(a.agent, b.agent));
      const primary = checkout.id === primaryId;
      const rank: LaneRank = primary && scope === "project" ? "primary" : rankOf(members);
      const lane: Lane = {
        id: checkout.id,
        project: workspace,
        checkout,
        primary,
        rank,
        nodes: members.map((value) => ({ ...value, column: 0 })),
        cleanup: cleanupOf(workspace, checkout),
        task: checkout.task_key ? (tasks.get(checkout.task_key) ?? null) : null,
      };
      if (lane.cleanup && rank === "resting") cleanup.push(lane);
      else if (members.length === 0 && !(primary && scope === "project")) empty.push(lane);
      else lanes.push(lane);
    }
  }
  lanes.sort((a, b) => RANK_ORDER[a.rank] - RANK_ORDER[b.rank] || Number(b.primary) - Number(a.primary) || latestActivity(b.nodes).localeCompare(latestActivity(a.nodes)));
  // Columns over the lanes in the order they are drawn, the folded cleanup
  // lanes last, so a line stays straight once they are unfolded too. A line
  // down to a later lane keeps its column free in every lane it crosses, so
  // it never runs through another agent's node (efs2).
  const drawn = [...lanes, ...cleanup];
  const laneOf = new Map<string, string>();
  const laneIndex = new Map<string, number>();
  drawn.forEach((lane, index) => {
    for (const node of lane.nodes) {
      laneOf.set(node.agent.pane_id, lane.id);
      laneIndex.set(node.agent.pane_id, index);
    }
  });
  const spans = agents.flatMap(({ agent }) => {
    const parent = agent.lineage_parent_pane_id;
    const from = parent ? laneIndex.get(parent) : undefined;
    const to = laneIndex.get(agent.pane_id);
    return parent && from !== undefined && to !== undefined && to > from + 1 ? [{ parent, from, to }] : [];
  });
  const column = new Map<string, number>();
  let columns = 0;
  drawn.forEach((lane, index) => {
    const crossing = new Set(spans.filter((span) => span.from < index && index < span.to).map((span) => column.get(span.parent)));
    let next = 0;
    for (const node of lane.nodes) {
      const parent = node.agent.lineage_parent_pane_id;
      const wanted = parent !== null && parent !== undefined ? column.get(parent) : undefined;
      if (wanted !== undefined && wanted >= next) {
        node.column = wanted;
      } else {
        node.column = next;
        while (crossing.has(node.column)) node.column += 1;
      }
      column.set(node.agent.pane_id, node.column);
      next = node.column + 1;
    }
    columns = Math.max(columns, next);
  });
  const delegations: Delegation[] = [];
  for (const { agent } of agents) {
    const parent = agent.lineage_parent_pane_id;
    const laneId = laneOf.get(agent.pane_id);
    if (!parent || !laneId || !laneOf.has(parent)) continue;
    delegations.push({ from: parent, to: agent.pane_id, within: laneOf.get(parent) === laneId });
  }
  return { lanes, empty, cleanup, delegations, columns };
}

// --- lineage -----------------------------------------------------------------

/** One agent in a lineage, at its depth's column and its row within the lineage. */
export type LineageNode = LensAgent & { depth: number; row: number };

export type Lineage = {
  rootPaneId: string;
  nodes: LineageNode[];
  rank: "turn" | "working" | "resting";
  cleanup: boolean;
};

export type LineageBoard = {
  lineages: Lineage[];
  /** Lineages whose agents all rest, folded as `쉬는 에이전트 N` (B25). */
  resting: Lineage[];
  /** Resting lineages rooted in a merged or folder-less worktree, folded as `정리할 것 N`. */
  cleanup: Lineage[];
  /** How many columns the deepest lineage needs: Observer, Implementor, then each level below. */
  columns: number;
  /** Parent to child inside one lineage, an arrow each (B23). */
  delegations: Delegation[];
};

/** A fold's count: every agent in its lineages. */
export function lineageAgentCount(lineages: readonly Lineage[]): number {
  return lineages.reduce((sum, lineage) => sum + lineage.nodes.length, 0);
}

/**
 * The lineage mode (D-06, B23, B25): one row per lineage, Observer, then
 * Implementor, then each level below left to right; an agent with no parent
 * in scope starts its own lineage in the first column. A lineage with an
 * agent whose turn it is stands first, then working ones, the most recently
 * active first inside each; lineages whose agents all rest fold into
 * `쉬는 에이전트` or, rooted in a worktree only there to be removed, into
 * `정리할 것`.
 */
export function buildLineages(agents: readonly LensAgent[]): LineageBoard {
  const byPane = new Map(agents.map((value) => [value.agent.pane_id, value]));
  const cleanupCheckouts = new Set(agents.filter((value) => cleanupOf(value.project, value.checkout) !== null).map((value) => value.checkout.id));
  const all: Lineage[] = [];
  let columns = 0;
  for (const root of agents) {
    const parent = root.agent.lineage_parent_pane_id;
    if (parent && byPane.has(parent)) continue;
    const nodes: LineageNode[] = [];
    const seen = new Set<string>();
    // A node's first child shares its row; each further child takes the next free one.
    const place = (value: LensAgent, depth: number, row: number): number => {
      seen.add(value.agent.pane_id);
      nodes.push({ ...value, depth, row });
      columns = Math.max(columns, depth + 1);
      const children = (value.agent.lineage_child_pane_ids ?? []).map((id) => byPane.get(id)).filter((child): child is LensAgent => child !== undefined && !seen.has(child.agent.pane_id));
      children.sort((a, b) => byBucketThenActivity(a.agent, b.agent));
      let used = 0;
      for (const child of children) used += place(child, depth + 1, row + used);
      return Math.max(1, used);
    };
    place(root, 0, 0);
    all.push({ rootPaneId: root.agent.pane_id, nodes, rank: rankOf(nodes), cleanup: cleanupCheckouts.has(root.checkout.id) });
  }
  all.sort((a, b) => RANK_ORDER[a.rank] - RANK_ORDER[b.rank] || latestActivity(b.nodes).localeCompare(latestActivity(a.nodes)));
  const delegations: Delegation[] = [];
  for (const lineage of all) {
    const inLineage = new Set(lineage.nodes.map((node) => node.agent.pane_id));
    for (const node of lineage.nodes) {
      const parent = node.agent.lineage_parent_pane_id;
      if (parent && inLineage.has(parent)) delegations.push({ from: parent, to: node.agent.pane_id, within: true });
    }
  }
  return {
    lineages: all.filter((lineage) => lineage.rank !== "resting"),
    resting: all.filter((lineage) => lineage.rank === "resting" && !lineage.cleanup),
    cleanup: all.filter((lineage) => lineage.rank === "resting" && lineage.cleanup),
    columns,
    delegations,
  };
}

/** The node's second line for an agent waiting on its children: `일하는 중 1 · 물음 1` (B21). */
export function childSummary(agent: AgentRow): string | null {
  const counts = agent.descendant_counts;
  if (!counts) return null;
  const asked = counts.question + counts.approval + counts.error;
  const parts = [
    counts.working > 0 ? `일하는 중 ${counts.working}` : null,
    asked > 0 ? `물음 ${asked}` : null,
    counts.done > 0 ? `끝남 ${counts.done}` : null,
  ].filter((part): part is string => part !== null);
  return parts.length > 0 ? parts.join(" · ") : null;
}

/** The lane to select when the Overview opens (D-17, B12): the checkout in front when it is this Project's, else the primary one. */
export function entryLane(workspace: Workspace | null | undefined, frontCheckoutId: string | null | undefined): string | null {
  if (!workspace) return null;
  const front = frontCheckoutId ? workspace.checkouts.find((checkout) => checkout.id === frontCheckoutId) : undefined;
  return front?.id ?? primaryCheckout(workspace)?.id ?? workspace.checkouts[0]?.id ?? null;
}
