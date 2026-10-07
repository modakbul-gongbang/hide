import type { AgentScope } from "./agentScope";
// The Overview's lenses (PRD overview-lenses-tiles-agents) as pure functions
// over the snapshot: the tiles that stand where the tab row was, and the
// agents each scope holds. The Agents graph itself is `agentGraph.ts`. Every
// value comes from the snapshot and a value not read yet is null, never a
// zero (design 10).

import type { TFunction } from "i18next";
import type { MessageKey } from "./i18n/catalogs";
import { formatRelativeTime } from "./i18n/format";
import type { InterfaceLanguage } from "./i18n/locale";
import type { DeviceAvailability } from "./navigation";
import { readFailureText, stageOf, type BoardProject, type PrBoard, type TasksBoard } from "./projectBoard";
import type { AgentRow, Checkout, ProjectSessions, Task, Workspace } from "./snapshot";

// --- agents ------------------------------------------------------------------

/**
 * Where an agent stands for the operator (D-37, D-38): its turn (asking, or
 * finished and not looked at), working, waiting on its children, or resting.
 * The graph's status chips and the Agents tile read this one order.
 */
export type AgentBucket = "turn" | "working" | "delegating" | "resting";

const BUCKET_LABEL: Record<AgentBucket, MessageKey> = {
  turn: "board.prGroup.turn",
  working: "requests.verb.working",
  delegating: "overview.bucket.delegating",
  resting: "requests.verb.idle",
};

const BUCKETS: readonly AgentBucket[] = ["turn", "working", "delegating", "resting"];

export function bucketOf(agent: AgentRow): AgentBucket {
  return agent.state.bucket;
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
    const byPane = new Map(agents.map((agent) => [agent.pane_id, agent]));
    const checkouts = new Map(workspace.checkouts.map((checkout) => [checkout.id, checkout]));
    const tasks = new Map((workspace.tasks?.tasks ?? []).map((task) => [task.key, task]));
    for (const member of workspace.agent_scope.members) {
      const agent = byPane.get(member.pane_id);
      const checkout = checkouts.get(member.checkout_id);
      if (!agent || !checkout) throw new Error("Overview scope references a missing agent or checkout");
      result.push({ agent, bucket: bucketOf(agent), project: workspace, checkout, device, task: checkout.task_key ? (tasks.get(checkout.task_key) ?? null) : null });
    }
  }
  return result;
}

// --- tiles -------------------------------------------------------------------

/** One stretch of a tile's bar, with the name and count its legend reads. */
export type TileSegment = { key: string; label: string; count: number };

export type TileId = "requests" | "agents" | "issues" | "prs" | "sessions";

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
  /** `label` names what the badge counts; the operator's turn when absent. */
  badge: { count: number; parts: TileSegment[]; label?: string } | null;
  bar: TileSegment[] | null;
  failure: string | null;
};

/**
 * The Agents tile (B3): how many agents the scope has, the badge of those
 * whose turn it is (asking, or finished and not looked at) split into
 * questions, approvals, errors and finished ones, and the bar of the four
 * buckets. A device that has not answered has no count.
 */
export function agentsTile(scope: AgentScope, availability: DeviceAvailability, t: TFunction<"translation">): Tile {
  const known = availability.state === "ready";
  const count = (bucket: AgentBucket) => scope.buckets[bucket];
  const turn = count("turn");
  const parts = [
    { key: "question", label: t("board.turn.question"), count: scope.turns.question },
    { key: "approval", label: t("overview.approval"), count: scope.turns.approval },
    { key: "error", label: t("common.error"), count: scope.turns.error },
    { key: "done", label: t("overview.finished"), count: scope.turns.done },
  ].filter((part) => part.count > 0);
  return {
    id: "agents",
    label: t("overview.agents"),
    value: known ? scope.overview_total : null,
    unit: null,
    badge: known && turn > 0 ? { count: turn, parts } : null,
    bar: known ? BUCKETS.map((bucket) => ({ key: bucket, label: t(BUCKET_LABEL[bucket]), count: count(bucket) })) : null,
    // The agents are the device's live rows, so a device that cannot answer
    // has no last value to keep: the ⚠ says why (B6), and the count stays empty.
    failure: availability.state === "unavailable" ? t("overview.agentsReadFailed", { reason: availability.text }) : null,
  };
}

/**
 * The Issues tile (B3): the open issues once the source has answered, and
 * the bar of where they stand, the backlog, in progress and in review. A
 * failed read keeps the last value and says so by the name (B6).
 */
export function issuesTile(board: TasksBoard, now: number, lastReadAt: number | null, language: InterfaceLanguage, t: TFunction<"translation">): Tile {
  const open = board.source.openIssues;
  const stage = (value: "backlog" | "working" | "review") => board.cards.filter((card) => card.task?.open && card.stage === value).length;
  const failure = board.source.failure ? `${readFailureText(board.source.failure, t)}${lastReadAt === null ? "" : ` · ${t("requests.staleValue", { age: ageWords(language, now - lastReadAt, t) })}`}` : null;
  return {
    id: "issues",
    label: t("overview.issues"),
    value: open,
    unit: t("board.unit.open"),
    badge: null,
    bar:
      open === null
        ? null
        : [
            { key: "backlog", label: t("board.stage.backlog"), count: stage("backlog") },
            { key: "working", label: t("board.stage.working"), count: stage("working") },
            { key: "review", label: t("board.stage.review"), count: stage("review") },
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
export function prsTile(board: PrBoard, t: TFunction<"translation">): Tile {
  const counts = board.counts;
  const parts = [
    { key: "review", label: t("board.stage.review"), count: counts.review },
    { key: "draft", label: t("overview.draft"), count: counts.draft },
    { key: "look", label: t("overview.reviewFinishedAgent"), count: counts.look },
  ].filter((part) => part.count > 0);
  return {
    id: "prs",
    label: t("overview.prs"),
    value: board.open,
    unit: t("board.unit.open"),
    badge: board.open !== null && counts.turn > 0 ? { count: counts.turn, parts } : null,
    bar:
      board.open === null
        ? null
        : [
            { key: "turn", label: t("board.prGroup.turn"), count: counts.turn },
            { key: "fixing", label: t("board.prGroup.fixing"), count: counts.fixing },
            { key: "blocked", label: t("board.prGroup.blocked"), count: counts.blocked },
          ],
    failure: board.failure ? readFailureText(board.failure, t) : null,
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
export function sessionsTile(sessions: ProjectSessions | null | undefined, workspaceId: string, now: number, t: TFunction<"translation">): Tile {
  const mine = sessions && sessions.workspace_id === workspaceId ? sessions : null;
  const failure = mine ? (mine.failure ?? mine.unavailable_reason) : null;
  const answered = mine !== null && !(mine.loading && mine.rows.length === 0) && !(failure && mine.rows.length === 0);
  const since = startOfDay(now);
  const today = answered ? mine.rows.filter((row) => row.updated_at_unix_ms >= since) : [];
  return {
    id: "sessions",
    label: t("overview.sessions"),
    value: answered ? today.length : null,
    unit: t("board.unit.today"),
    badge: null,
    bar: answered
      ? [
          { key: "claude", label: "Claude", count: today.filter((row) => row.provider === "claude").length },
          { key: "codex", label: "Codex", count: today.filter((row) => row.provider === "codex").length },
        ]
      : null,
    failure: failure ? t("overview.sessionsReadFailed", { value: mine?.rows.length ? t("overview.lastReadValue") : t("board.noValue") }) : null,
  };
}

/** The age a failure's popover gives its last value: the language's own relative form, and `overview.justNow` under a minute. */
export function ageWords(language: InterfaceLanguage, ms: number, t: TFunction<"translation">): string {
  const minutes = Math.max(0, Math.floor(ms / 60_000));
  if (minutes < 1) return t("overview.justNow");
  if (minutes < 60) return formatRelativeTime(language, -minutes, "minute");
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return formatRelativeTime(language, -hours, "hour");
  return formatRelativeTime(language, -Math.floor(hours / 24), "day");
}

// --- worktree cleanup --------------------------------------------------------

/** Why a worktree is only there to be removed: its work is merged, or its folder is gone. */
export type Cleanup = "merged" | "missing";

export function cleanupOf(workspace: Workspace, checkout: Checkout): Cleanup | null {
  if (!checkout.is_worktree || checkout.is_primary === true || workspace.is_git !== true) return null;
  if (!checkout.exists || checkout.worktree?.missing === true) return "missing";
  return stageOf(checkout) === "done" ? "merged" : null;
}
