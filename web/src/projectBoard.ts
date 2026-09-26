// The Overview's Tasks and Agents boards (PRD task-agents-views D-03..D-13)
// as pure functions over the snapshot, for one Project or for All projects.
// A card is a task: a task of the project's source with the checkout that
// works on it, a checkout with no task (an untracked one), or an open task no
// checkout works on (the backlog). Git decides the stage; agents and the
// task's own state never move it. Every value drawn is one the snapshot
// carries, and a count the source has not answered for is left out rather
// than drawn as zero (design 10).

import type { AgentRow, Checkout, PullRequest, Task, Workspace } from "./snapshot";

export type Stage = "backlog" | "ready" | "working" | "review" | "done";

export const STAGES: readonly { stage: Stage; label: string }[] = [
  { stage: "backlog", label: "백로그" },
  { stage: "ready", label: "준비" },
  { stage: "working", label: "진행 중" },
  { stage: "review", label: "리뷰" },
  { stage: "done", label: "완료" },
];

/** A checkout's stage: only a task nobody works on is in the backlog. */
export type GitStage = Exclude<Stage, "backlog">;

/** A checkout's Git stage: merged, then an open pull request, then local work, else ready. Agents and the task's state never move it. */
export function stageOf(checkout: Checkout): GitStage {
  const pr = checkout.pull_request;
  if (checkout.worktree?.merged === true || pr?.badge === "merged") return "done";
  if (pr && pr.badge !== "closed") return "review";
  if ((checkout.changed_file_count ?? 0) > 0 || (checkout.ahead ?? 0) > 0) return "working";
  return "ready";
}

export type BoardRow = {
  agent: AgentRow;
  /** 0 for a root, one more per delegation step. */
  depth: number;
};

/** A pull request's lifecycle colour (D-06): open green, draft grey, merged purple, closed red. */
export type PrTone = "open" | "draft" | "merged" | "closed";

/** The result a card delivers: its pull request, with the CI rollup when it was read. */
export type PrChip = {
  number: number;
  url: string;
  tone: PrTone;
  checks: "passing" | "failed" | "pending" | null;
};

export function prChip(pr: PullRequest): PrChip {
  const tone: PrTone = pr.badge === "merged" ? "merged" : pr.badge === "closed" ? "closed" : pr.is_draft ? "draft" : "open";
  const checks = pr.checks === "passing" || pr.checks === "failed" || pr.checks === "pending" ? pr.checks : null;
  return { number: pr.number, url: pr.url, tone, checks };
}

/** The delivery facts a card states (D-04), each only when the snapshot has it and it is above zero. */
export type Facts = {
  /** Changed files, on a card in progress. */
  files: number | null;
  /** Commits ahead of the base, on a card in progress. */
  ahead: number | null;
  pr: PrChip | null;
  /** Commits the upstream has that the branch does not, on any card. */
  behind: number | null;
};

/** Where a card or agent lives: its Project, for All projects' cards. */
export type BoardPlace = { projectId: string; projectLabel: string };

export type TaskCard = {
  id: string;
  place: BoardPlace;
  /** The checkout the card opens, or null for a backlog task. */
  checkout: Checkout | null;
  /** The task, or null for an untracked checkout. */
  task: Task | null;
  /** The column, or null for a card on the ad hoc strip. */
  stage: Stage | null;
  /** The task's title, else the checkout's branch. */
  title: string;
  facts: Facts;
  /** Every agent working in the checkout, each lineage root first. */
  rows: BoardRow[];
  /** At most two agents, the ones that need the operator first (D-04). */
  shown: AgentRow[];
  /** How many more agents than `shown` work here. */
  more: number;
  needsYou: boolean;
  /** A row reports an error; the halo is drawn in danger instead of warning. */
  error: boolean;
  /** A ready card with no agent on this machine: hover or focus offers Start agent (D-05). */
  canStart: boolean;
  /** The id's tooltip: where the task comes from and the branch working on it (D-05). */
  idHelp: string;
  /** Why the card's source could not be read, drawn as a small mark with this tooltip (D-13). */
  sourceFailure: string | null;
  /** The open tasks this one waits on, the lock line (D-04): each by its id, else its title when the scope has it. */
  blockedBy: Blocker[];
};

export type Blocker = { key: string; label: string };

export type AgentColumn = "active" | "done" | "seen";

export const AGENT_COLUMNS: readonly { column: AgentColumn; label: string; groups: readonly string[] }[] = [
  { column: "active", label: "진행 중", groups: ["working", "needs_you"] },
  { column: "done", label: "내 확인 대기", groups: ["done"] },
  { column: "seen", label: "끝", groups: ["seen"] },
];

/** One agent on the Agents board (D-12). */
export type AgentCard = {
  agent: AgentRow;
  /** 0 for a root, one more per delegation step; a delegated agent sits under its parent. */
  depth: number;
  place: BoardPlace;
  /** The SSH device the agent runs on, or null for this machine. */
  device: string | null;
  checkout: Checkout | null;
  /** The checkout, and on All projects its Project before it. */
  where: string | null;
  task: Task | null;
  /** The task chip's tooltip: title, branch, and the pull request, the one place a PR shows here. */
  taskHelp: string | null;
};

/** A Project the board reads: its catalog row, every agent its device reported, and the device's name when it is not this machine. */
export type BoardProject = { workspace: Workspace; agents: AgentRow[]; device: string | null };

export type BoardScope = "project" | "all";

/** A Project with agents and no task source, gathered under All projects' board (D-10). */
export type Unconnected = { place: BoardPlace; agents: number; reason: string | null };

export type TasksBoard = {
  /** Nothing to draw: no source and no agent (B14). */
  empty: boolean;
  /** Whether the stage columns are drawn: a Git project is in scope. */
  columns: boolean;
  cards: TaskCard[];
  /** Checkouts that are not linked worktrees (the primary checkout, a folder), while an agent works in them. */
  adHoc: TaskCard[];
  unconnected: Unconnected[];
  /** A source listed more than the core keeps, so the backlog may hold more than it shows. */
  overflow: boolean;
  /** Why no source is connected, for the empty state's connect control. */
  unconnectedReason: string | null;
};

export type AgentsBoard = { cards: AgentCard[] };

/** How much an agent needs the operator; lower first. */
function attention(agent: AgentRow): number {
  if (agent.group === "needs_you") return agent.demand === "error" ? 0 : 1;
  if (agent.group === "done") return 2;
  if (agent.group === "working") return 3;
  return 4;
}

/** The agents a card names: the two that need the operator most, in the core's order otherwise. */
export function shownAgents(rows: BoardRow[]): { shown: AgentRow[]; more: number } {
  const ranked = rows
    .map((row, index) => ({ agent: row.agent, index }))
    .sort((a, b) => attention(a.agent) - attention(b.agent) || a.index - b.index)
    .map(({ agent }) => agent);
  return { shown: ranked.slice(0, 2), more: Math.max(0, ranked.length - 2) };
}

/** Needs-you cards first, each group in its original order; nothing else reorders a column. */
function prioritized<T extends { needsYou: boolean }>(cards: T[]): T[] {
  return cards
    .map((value, index) => ({ value, index }))
    .sort((a, b) => (a.value.needsYou === b.value.needsYou ? a.index - b.index : a.value.needsYou ? -1 : 1))
    .map(({ value }) => value);
}

/**
 * Which checkout owns each pane, and a lineage walker over `agents`: a list
 * of agents drawn with their whole lineage, root first, whatever the sidebar
 * has folded, so its rows carry no descendant badge.
 */
function lineage(workspace: Workspace, agents: AgentRow[]) {
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
  // A checkout's rows: its agents, each lineage from the first ancestor that
  // is not also working here.
  const checkoutRows = (checkout: Checkout): BoardRow[] => {
    const local = agents.filter((agent) => owners.get(agent.pane_id)?.id === checkout.id);
    const localIds = new Set(local.map((agent) => agent.pane_id));
    return treeRows(local.filter((agent) => !agent.lineage_parent_pane_id || !localIds.has(agent.lineage_parent_pane_id)));
  };
  return { owners, byPane, treeRows, checkoutRows };
}

/**
 * Each checkout's agent rows by checkout id, the rows the sidebar draws under
 * an opened checkout. `agents` is every agent of the project's device.
 */
export function checkoutAgentRows(workspace: Workspace, agents: AgentRow[]): Map<string, BoardRow[]> {
  const { checkoutRows } = lineage(workspace, agents);
  return new Map(workspace.checkouts.map((checkout) => [checkout.id, checkoutRows(checkout)]));
}

function place(workspace: Workspace): BoardPlace {
  return { projectId: workspace.id, projectLabel: workspace.label };
}

function facts(checkout: Checkout | null, stage: Stage | null): Facts {
  const working = stage === "working";
  const files = checkout?.changed_file_count ?? 0;
  const ahead = checkout?.ahead ?? 0;
  const behind = checkout?.worktree?.behind_upstream ?? 0;
  return {
    files: working && files > 0 ? files : null,
    ahead: working && ahead > 0 ? ahead : null,
    pr: checkout?.pull_request ? prChip(checkout.pull_request) : null,
    behind: behind > 0 ? behind : null,
  };
}

/** The sentence a failed source read carries on its cards, with its age (B13). */
function sourceFailure(workspace: Workspace, now: number): string | null {
  const source = workspace.tasks?.source;
  if (!source?.failure) return null;
  const age = source.last_read_at_unix_ms == null ? null : Math.max(0, Math.floor((now - source.last_read_at_unix_ms) / 60_000));
  return [`${source.label} 읽기 실패`, age === null ? "마지막으로 확인한 상태" : `${age}분 전에 확인한 상태`, "자세한 오류는 진단 로그"].join(" · ");
}

function card(
  id: string,
  workspace: Workspace,
  scope: BoardScope,
  checkout: Checkout | null,
  task: Task | null,
  stage: Stage | null,
  rows: BoardRow[],
  now: number,
): TaskCard {
  const { shown, more } = shownAgents(rows);
  const branch = checkout?.branch ?? checkout?.label ?? null;
  const sourceLabel = task ? (workspace.tasks?.source?.label ?? task.source) : null;
  return {
    id,
    place: place(workspace),
    checkout,
    task,
    stage,
    title: task?.title ?? branch ?? "",
    facts: facts(checkout, stage),
    rows,
    shown,
    more,
    needsYou: rows.some((row) => row.agent.group === "needs_you"),
    error: rows.some((row) => row.agent.demand === "error"),
    canStart: stage === "ready" && rows.length === 0 && checkout !== null && !workspace.remote_target_id,
    idHelp: [sourceLabel, scope === "all" ? workspace.label : null, branch].filter(Boolean).join(" · "),
    sourceFailure: task ? sourceFailure(workspace, now) : null,
    blockedBy: [],
  };
}

/** Names each card's blockers once every project's tasks are known, since a blocker may be another project's task (D-10). */
function nameBlockers(cards: TaskCard[], titles: Map<string, string>) {
  for (const value of cards) {
    value.blockedBy = (value.task?.blocked_by ?? []).map((ref) => ({ key: ref.key, label: ref.id ?? titles.get(ref.key) ?? ref.key }));
  }
}

/** The Tasks board for one Project or for All projects (D-03, D-07, D-10). */
export function buildTasks(projects: readonly BoardProject[], scope: BoardScope, now: number): TasksBoard {
  const cards: TaskCard[] = [];
  const adHoc: TaskCard[] = [];
  const unconnected: Unconnected[] = [];
  let overflow = false;
  let connected = false;
  let agentsAnywhere = false;
  let unconnectedReason: string | null = null;
  const titles = new Map<string, string>();
  for (const { workspace, agents } of projects) {
    const { checkoutRows } = lineage(workspace, agents);
    for (const task of workspace.tasks?.tasks ?? []) titles.set(task.key, task.title);
    const source = workspace.tasks?.source ?? null;
    const tasks = new Map((workspace.tasks?.tasks ?? []).map((task) => [task.key, task]));
    const working = workspace.checkouts.reduce((total, checkout) => total + checkoutRows(checkout).length, 0);
    agentsAnywhere ||= working > 0;
    connected ||= source !== null;
    overflow ||= workspace.tasks?.overflow ?? false;
    unconnectedReason ??= workspace.tasks?.unconnected_reason ?? null;
    // All projects gathers a Project with no source under the board rather
    // than spreading its untracked checkouts through the columns (D-10).
    if (scope === "all" && source === null) {
      if (working > 0) unconnected.push({ place: place(workspace), agents: working, reason: workspace.tasks?.unconnected_reason ?? null });
      continue;
    }
    const git = workspace.is_git === true;
    const worked = new Set<string>();
    for (const checkout of workspace.checkouts) {
      const rows = checkoutRows(checkout);
      const task = checkout.task_key ? (tasks.get(checkout.task_key) ?? null) : null;
      if (!(checkout.is_worktree && git)) {
        if (task) worked.add(task.key);
        if (rows.length > 0) adHoc.push(card(`checkout:${checkout.id}`, workspace, scope, checkout, task, null, rows, now));
        continue;
      }
      const stage = stageOf(checkout);
      if (task) worked.add(task.key);
      cards.push(card(`checkout:${checkout.id}`, workspace, scope, checkout, task, stage, rows, now));
      // Another task the same pull request closes shows that pull request too (D-07).
      for (const key of checkout.closes_task_keys ?? []) {
        const closed = tasks.get(key);
        if (!closed || worked.has(key)) continue;
        worked.add(key);
        cards.push(card(`closes:${checkout.id}:${key}`, workspace, scope, checkout, closed, stage, [], now));
      }
    }
    for (const task of tasks.values()) {
      if (!task.open || worked.has(task.key)) continue;
      cards.push(card(`task:${workspace.id}:${task.key}`, workspace, scope, null, task, "backlog", [], now));
    }
  }
  nameBlockers(cards, titles);
  nameBlockers(adHoc, titles);
  return {
    empty: !connected && !agentsAnywhere,
    columns: projects.some(({ workspace }) => workspace.is_git === true),
    cards: prioritized(cards),
    adHoc: prioritized(adHoc),
    unconnected,
    overflow,
    unconnectedReason,
  };
}

/** The cards of one Tasks column. */
export function stageCards(board: TasksBoard, stage: Stage): TaskCard[] {
  return board.cards.filter((value) => value.stage === stage);
}

/** One arrow of the Dependencies mode: from the blocker's card to the card it blocks. */
export type DependencyEdge = { from: string; to: string };

export type DependencyGraph = {
  /** The tasks that wait on or block another in scope, in columns left to right by how many blockers precede them. */
  layers: TaskCard[][];
  edges: DependencyEdge[];
  /** The tasks with no relation in scope, gathered below the graph (D-09). */
  unrelated: TaskCard[];
};

/**
 * The Dependencies mode (D-09, D-10): the Board's task cards, laid out left
 * to right. A card's column is the longest chain of blockers before it, and
 * each column is ordered by the mean row of the blockers it hangs from, so
 * arrows mostly run straight. Keys name tasks across projects, so a blocker in
 * another project of the scope is an arrow too; one outside the scope is only
 * the lock line. Untracked checkouts are Board-only. A cycle, which a source
 * should not allow, drops the arrow that closes it rather than looping.
 */
export function buildDependencies(board: TasksBoard): DependencyGraph {
  const byKey = new Map<string, TaskCard>();
  for (const value of board.cards) if (value.task && !byKey.has(value.task.key)) byKey.set(value.task.key, value);
  const nodes = [...byKey.values()];
  const blockers = new Map<string, TaskCard[]>();
  const edges: DependencyEdge[] = [];
  const related = new Set<string>();
  for (const value of nodes) {
    const before = value.blockedBy.flatMap((blocker) => {
      const from = byKey.get(blocker.key);
      return from && from !== value ? [from] : [];
    });
    blockers.set(value.id, before);
    for (const from of before) {
      edges.push({ from: from.id, to: value.id });
      related.add(from.id);
      related.add(value.id);
    }
  }
  const depth = new Map<string, number>();
  const visiting = new Set<string>();
  const depthOf = (value: TaskCard): number => {
    const known = depth.get(value.id);
    if (known !== undefined) return known;
    if (visiting.has(value.id)) return -1;
    visiting.add(value.id);
    const found = Math.max(-1, ...(blockers.get(value.id) ?? []).map(depthOf)) + 1;
    visiting.delete(value.id);
    depth.set(value.id, found);
    return found;
  };
  const layers: TaskCard[][] = [];
  for (const value of nodes) {
    if (!related.has(value.id)) continue;
    const layer = depthOf(value);
    (layers[layer] ??= []).push(value);
  }
  const row = new Map<string, number>();
  const dense = layers.filter((layer) => layer !== undefined);
  for (const layer of dense) {
    const weight = (value: TaskCard) => {
      const rows = (blockers.get(value.id) ?? []).flatMap((from) => (row.has(from.id) ? [row.get(from.id) as number] : []));
      return rows.length > 0 ? rows.reduce((sum, at) => sum + at, 0) / rows.length : Number.POSITIVE_INFINITY;
    };
    const ordered = layer.map((value, index) => ({ value, index, weight: weight(value) })).sort((a, b) => a.weight - b.weight || a.index - b.index);
    layer.splice(0, layer.length, ...ordered.map(({ value }) => value));
    layer.forEach((value, index) => row.set(value.id, index));
  }
  const forward = edges.filter((edge) => (depth.get(edge.from) ?? 0) < (depth.get(edge.to) ?? 0));
  return { layers: dense, edges: forward, unrelated: nodes.filter((value) => !related.has(value.id)) };
}

/**
 * The Agents board (D-12): every agent of the scope, a delegated one right
 * under its parent, one step in, in its root's column. `agents` is every
 * agent of each Project's device, since a descendant may work in another
 * Project's checkout; only the Project's own panes decide where a root sits.
 */
export function buildAgents(projects: readonly BoardProject[], scope: BoardScope): AgentsBoard {
  const cards: AgentCard[] = [];
  for (const { workspace, agents, device } of projects) {
    const { owners, byPane, treeRows } = lineage(workspace, agents);
    const tasks = new Map((workspace.tasks?.tasks ?? []).map((task) => [task.key, task]));
    const roots = agents.filter((agent) => {
      const parent = agent.lineage_parent_pane_id;
      if (parent && byPane.has(parent)) return false;
      return owners.has(agent.pane_id);
    });
    for (const row of treeRows(roots)) {
      const checkout = owners.get(row.agent.pane_id) ?? null;
      const branch = checkout?.branch ?? checkout?.label ?? null;
      const task = checkout?.task_key ? (tasks.get(checkout.task_key) ?? null) : null;
      const pr = checkout?.pull_request ?? null;
      cards.push({
        agent: row.agent,
        depth: row.depth,
        place: place(workspace),
        device,
        checkout,
        where: scope === "all" ? [workspace.label, branch].filter(Boolean).join(" · ") : branch,
        task,
        taskHelp: task ? [task.title, branch, pr ? `PR #${pr.number}` : null].filter(Boolean).join(" · ") : null,
      });
    }
  }
  return { cards };
}

/**
 * The cards of one Agents column: each root by its own group, and its
 * descendants with it, since a delegated agent is only ever Working or Seen
 * and reads under the parent it answers to.
 */
export function agentColumnCards(board: AgentsBoard, column: AgentColumn): AgentCard[] {
  const groups = AGENT_COLUMNS.find((row) => row.column === column)?.groups ?? [];
  const cards: AgentCard[] = [];
  let rootIn = false;
  for (const value of board.cards) {
    if (value.depth === 0) rootIn = groups.includes(value.agent.group);
    if (rootIn) cards.push(value);
  }
  return cards;
}

/** One row of the waiting band (D-11): an agent that waits on the operator, where it works, and its request. */
export type WaitingRow = {
  agent: AgentRow;
  place: BoardPlace;
  /** Mono context before the request: the project on All projects, the task's id, the branch. */
  where: string;
};

/**
 * The waiting band (D-11, B10): every agent of the scope that waits on the
 * operator, the ones asking first (an error before a question or approval),
 * then the finished ones not yet looked at, each group in the core's order.
 * Only an agent in the Project's own checkouts counts; a delegated agent is
 * never Needs You or unread, so its parent is the one that waits.
 */
export function buildWaiting(projects: readonly BoardProject[], scope: BoardScope): WaitingRow[] {
  const rows: (WaitingRow & { rank: number; index: number })[] = [];
  for (const { workspace, agents } of projects) {
    const { owners } = lineage(workspace, agents);
    const tasks = new Map((workspace.tasks?.tasks ?? []).map((task) => [task.key, task]));
    for (const agent of agents) {
      const checkout = owners.get(agent.pane_id);
      if (!checkout || (agent.group !== "needs_you" && agent.group !== "done")) continue;
      const task = checkout.task_key ? tasks.get(checkout.task_key) : undefined;
      const where = [scope === "all" ? workspace.label : null, task?.id ?? null, checkout.branch ?? checkout.label].filter(Boolean).join(" · ");
      rows.push({ agent, place: place(workspace), where, rank: attention(agent), index: rows.length });
    }
  }
  return rows.sort((a, b) => a.rank - b.rank || a.index - b.index).map(({ agent, place: at, where }) => ({ agent, place: at, where }));
}

export type BoardStats = {
  worktrees: number;
  /** Open pull requests, or null until GitHub has answered for this project. */
  openPullRequests: number | null;
  /** The primary checkout's branch and how far origin is ahead of it, only when it is. */
  behind: { branch: string; count: number } | null;
  /** Linked worktrees whose work is merged, the ones the Done column lists for removal. */
  merged: number;
  /**
   * Allocated disk in bytes, `measuring` while the core walks it, or null when
   * there is no number: never asked, or a part could not be read.
   */
  disk: number | "measuring" | null;
};

/** A Git project's facts line (B2); the All projects scope sums them across projects. */
export function projectStats(workspace: Workspace): BoardStats {
  const answered = workspace.checkouts.some((checkout) => checkout.github?.last_success_at_unix_ms != null);
  const primary = workspace.checkouts.find((checkout) => checkout.worktree?.is_main) ?? workspace.checkouts.find((checkout) => !checkout.is_worktree) ?? null;
  const behind = primary?.worktree?.behind_upstream ?? 0;
  return {
    worktrees: workspace.checkouts.filter((checkout) => checkout.is_worktree).length,
    openPullRequests: answered ? workspace.checkouts.filter((checkout) => checkout.pull_request?.badge === "open" || checkout.pull_request?.badge === "review").length : null,
    behind: primary && behind > 0 ? { branch: primary.branch ?? primary.label, count: behind } : null,
    merged: workspace.checkouts.filter((checkout) => checkout.is_worktree && stageOf(checkout) === "done").length,
    disk: workspace.disk?.total_bytes ?? (workspace.disk?.measuring ? "measuring" : null),
  };
}

export type AllProjectsStats = {
  projects: number;
  /** Open pull requests across every Project, or null while any Git project has no answer from GitHub. */
  openPullRequests: number | null;
  /** Merged worktrees across every Project, or null while any Project's catalog row is missing. */
  merged: number | null;
};

/**
 * The All projects facts line: the Project count, and a total only when every
 * Project can give its part. A device that has not answered leaves a Project
 * without its row, and a Git project GitHub has not answered for has no PR
 * count; a repository with no GitHub remote has none to count.
 */
export function allProjectsStats(workspaces: readonly (Workspace | null)[]): AllProjectsStats {
  const known = workspaces.every((workspace) => workspace !== null);
  let openPullRequests: number | null = 0;
  let merged = 0;
  for (const workspace of workspaces) {
    if (!workspace) continue;
    const stats = projectStats(workspace);
    merged += stats.merged;
    if (!workspace.is_git || workspace.checkouts.some((checkout) => checkout.github?.failure_category === "no GitHub remote")) continue;
    openPullRequests = openPullRequests === null || stats.openPullRequests === null ? null : openPullRequests + stats.openPullRequests;
  }
  return { projects: workspaces.length, openPullRequests: known ? openPullRequests : null, merged: known ? merged : null };
}

/** A size the way the Swift Overview writes it: `812 MB`, `1.4 GB`, binary units. */
export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  if (unit === 0) return `${Math.trunc(value)} B`;
  return `${value < 10 ? value.toFixed(1) : value.toFixed(0)} ${units[unit]}`;
}
