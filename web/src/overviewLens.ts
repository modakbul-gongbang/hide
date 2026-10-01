// The Overview's lenses (PRD overview-lenses-tiles-agents) as pure functions
// over the snapshot: the tiles that stand where the tab row was, and the
// agents each scope holds. The Agents graph itself is `agentGraph.ts`. Every
// value comes from the snapshot and a value not read yet is null, never a
// zero (design 10).

import type { DeviceAvailability } from "./navigation";
import { stageOf, type BoardProject, type PrBoard, type TasksBoard } from "./projectBoard";
import type { AgentRow, Checkout, ProjectSessions, Task, Workspace } from "./snapshot";

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

export function bucketOf(agent: AgentRow): AgentBucket {
  if (agent.group === "needs_you" || agent.group === "done") return "turn";
  if (agent.waiting_on_descendants) return "delegating";
  if (agent.group === "working") return "working";
  return "resting";
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

export type TileId = "agents" | "issues" | "prs" | "sessions";

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

/**
 * The PRs tile (PRD overview-lenses-prs B1): the open pull requests once
 * GitHub has answered, the badge of the ones waiting on the operator split
 * into review, drafts and finished agents to look at, and the bar of the
 * operator's turn, an agent fixing and the blocked ones. A failed read keeps
 * the last value and says so by the name (B22).
 */
export function prsTile(board: PrBoard): Tile {
  const rows = (group: string) => board.groups.find((entry) => entry.group === group)?.rows ?? [];
  const turn = rows("turn");
  const look = turn.filter((row) => row.needsLook).length;
  const drafts = turn.filter((row) => !row.needsLook && row.tone === "draft").length;
  const parts = [
    { key: "review", label: "리뷰", count: turn.length - look - drafts },
    { key: "draft", label: "초안", count: drafts },
    { key: "look", label: "끝난 에이전트 확인", count: look },
  ].filter((part) => part.count > 0);
  return {
    id: "prs",
    label: "PRs",
    value: board.open,
    unit: "열림",
    badge: board.open !== null && turn.length > 0 ? { count: turn.length, parts } : null,
    bar:
      board.open === null
        ? null
        : [
            { key: "turn", label: "내 차례", count: turn.length },
            { key: "fixing", label: "에이전트가 고치는 중", count: rows("fixing").length },
            { key: "blocked", label: "CI 실패 · 맡은 에이전트 없음", count: rows("blocked").length },
          ],
    failure: board.failure,
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

// --- worktree cleanup --------------------------------------------------------

/** Why a worktree is only there to be removed: its work is merged, or its folder is gone. */
export type Cleanup = "merged" | "missing";

export function cleanupOf(workspace: Workspace, checkout: Checkout): Cleanup | null {
  if (!checkout.is_worktree || checkout.is_primary === true || workspace.is_git !== true) return null;
  if (!checkout.exists || checkout.worktree?.missing === true) return "missing";
  return stageOf(checkout) === "done" ? "merged" : null;
}
