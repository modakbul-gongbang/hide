// The Overview's Issues view (PRD task-agents-views, reworked issue-first
// on 2026-09-28 and issues-only by PRD overview-lenses-issues) as pure
// functions over the snapshot, for one Project or for All projects. Every
// card is an issue: the checkout and agents working on it and the pull
// request they opened ride on it, and work with no issue folds into one line
// per column (D-07). Git decides the stage; agents and the issue's own state
// never move it. Every value drawn is one the snapshot carries, and a count
// the source has not answered for is left out rather than drawn as zero
// (design 10).

import type { AgentRow, Checkout, PullRequest, Task, Workspace } from "./snapshot";

export type Stage = "backlog" | "working" | "review" | "done";

export const STAGES: readonly { stage: Stage; label: string }[] = [
  { stage: "backlog", label: "백로그" },
  { stage: "working", label: "진행 중" },
  { stage: "review", label: "리뷰" },
  { stage: "done", label: "완료" },
];

/** A checkout's stage: only an issue nobody works on is in the backlog. */
export type GitStage = Exclude<Stage, "backlog">;

/** A checkout's Git stage: merged, then an open pull request, else work in progress. Agents and the issue's state never move it. */
export function stageOf(checkout: Checkout): GitStage {
  const pr = checkout.pull_request;
  if (checkout.worktree?.merged === true || pr?.badge === "merged") return "done";
  if (pr && pr.badge !== "closed") return "review";
  return "working";
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
  /** The review GitHub asks for, on an open pull request only. */
  review: "review_required" | "changes_requested" | "approved" | null;
};

export function prChip(pr: PullRequest): PrChip {
  const tone: PrTone = pr.badge === "merged" ? "merged" : pr.badge === "closed" ? "closed" : pr.is_draft ? "draft" : "open";
  const checks = pr.checks === "passing" || pr.checks === "failed" || pr.checks === "pending" ? pr.checks : null;
  return { number: pr.number, url: pr.url, tone, checks, review: tone === "open" ? pr.review : null };
}

/** Where an issue's work is (B2): its branch, commits ahead of the base, and changed files, each above zero only. */
export type CheckoutChip = {
  branch: string;
  /** The primary checkout or a folder, drawn with a house. */
  primary: boolean;
  ahead: number | null;
  files: number | null;
};

/** Where a card or agent lives: its Project, for All projects' cards. */
export type BoardPlace = { projectId: string; projectLabel: string };

/** What a card's hover slot offers first (B6): 시작 on the backlog, the Workspace in progress, the pull request in review. */
export type FirstAction = "start" | "workspace" | "pull_request" | null;

/**
 * One issue card (D-07, D-09). Its head is always the issue; the checkout
 * working on it, the pull request it delivered and its agents ride under it.
 */
export type TaskCard = {
  id: string;
  place: BoardPlace;
  /** The Project the issue belongs to, for its checkout card and its device. */
  owner: Workspace;
  /** The Project's name beside the id, on All projects only. */
  project: string | null;
  /** The checkout working on the issue, or null for a backlog issue. */
  checkout: Checkout | null;
  task: Task;
  stage: Stage;
  title: string;
  /** The checkout chip, in progress and in review. */
  chip: CheckoutChip | null;
  pr: PrChip | null;
  /** When the pull request that closed the issue merged, for the done line's popover. */
  mergedAt: number | null;
  /** Every agent working in the checkout, each lineage root first. */
  rows: BoardRow[];
  /** At most two agents, the ones that need the operator first. */
  shown: AgentRow[];
  /** How many more agents than `shown` work here. */
  more: number;
  /** An agent asks or finished and was not looked at: the only coloured card (B5). */
  needsYou: boolean;
  /** An open backlog issue of a project on this Mac: 시작 opens the Start dialog. */
  canStart: boolean;
  first: FirstAction;
  /** A Local issue: its title and body are edited in its panel (D-41). */
  editable: boolean;
  /** The id's hint: where the issue comes from and the branch working on it. */
  idHelp: string;
  /** Why the card's source could not be read and how old its value is (B17). */
  sourceFailure: string | null;
  /** The open tasks this one waits on, the lock line: each by its id, else its title when the scope has it. */
  blockedBy: Blocker[];
  /** When the issue last changed, or null; the backlog's order. */
  updatedAt: number | null;
};

/** A worktree with no issue, one of `이슈 없는 워크트리 N` under 진행 중 (B4). */
export type LooseWorktree = { branch: string };

/** An open pull request with no issue, one of `이슈 없는 PR N` under 리뷰 (B4). */
export type LoosePullRequest = { owner: Workspace; number: number };

export type Blocker = { key: string; label: string };

/** A Project the board reads: its catalog row, every agent its device reported, and the device's name when it is not this machine. */
export type BoardProject = { workspace: Workspace; agents: AgentRow[]; device: string | null };

export type BoardScope = "project" | "all";

/** How the scope's issue sources stand, for the backlog's header and the facts line. */
export type SourceState = {
  /** A source is still answering its first read. */
  reading: boolean;
  /** That a source could not be read (the first one), for a mark beside 백로그; the reason is in the diagnostic log (design 13). */
  failure: string | null;
  /** Open issues across the scope, once every source has answered. */
  openIssues: number | null;
  /** The one source label when the scope has one (`GitHub`, `Local`). */
  label: string | null;
};

export type TasksBoard = {
  cards: TaskCard[];
  /** Work with no issue, folded into one line per column (B4). */
  loose: { worktrees: LooseWorktree[]; pullRequests: LoosePullRequest[] };
  /** A source listed more than the core keeps, so the backlog may hold more than it shows. */
  overflow: boolean;
  source: SourceState;
};

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
 * Each checkout's agent rows by checkout id, the rows the sidebar draws under
 * an opened checkout and a task card lists. `agents` is every agent of the
 * project's device. A checkout's rows are its agents, each drawn with its
 * whole lineage from the first ancestor that is not also working here, root
 * first, whatever is folded; the Projects sidebar drops a folded row's
 * descendants itself (`unfoldedRows`).
 */
export function checkoutAgentRows(workspace: Workspace, agents: AgentRow[]): Map<string, BoardRow[]> {
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
  const checkoutRows = (checkout: Checkout): BoardRow[] => {
    const local = agents.filter((agent) => owners.get(agent.pane_id)?.id === checkout.id);
    const localIds = new Set(local.map((agent) => agent.pane_id));
    return treeRows(local.filter((agent) => !agent.lineage_parent_pane_id || !localIds.has(agent.lineage_parent_pane_id)));
  };
  return new Map(workspace.checkouts.map((checkout) => [checkout.id, checkoutRows(checkout)]));
}

function place(workspace: Workspace): BoardPlace {
  return { projectId: workspace.id, projectLabel: workspace.label };
}

function chipOf(checkout: Checkout | null, stage: Stage): CheckoutChip | null {
  if (!checkout || (stage !== "working" && stage !== "review")) return null;
  const files = checkout.changed_file_count ?? 0;
  const ahead = checkout.ahead ?? 0;
  return {
    branch: checkout.branch ?? checkout.label,
    primary: checkout.is_primary === true || !checkout.is_worktree,
    ahead: ahead > 0 ? ahead : null,
    files: files > 0 ? files : null,
  };
}

/** What a failed source read says, with the value's age (B17); the reason is in the diagnostic log. */
function sourceFailure(workspace: Workspace, now: number): string | null {
  const source = workspace.tasks?.source;
  if (!source?.failure) return null;
  const age = source.last_read_at_unix_ms == null ? null : Math.max(0, Math.floor((now - source.last_read_at_unix_ms) / 60_000));
  return [`${source.label} 읽기 실패`, age === null ? "마지막 값" : `${age}분 전 값`, "이유는 로그에"].join(" · ");
}

function firstAction(stage: Stage, canStart: boolean, checkout: Checkout | null, pr: PrChip | null): FirstAction {
  if (stage === "backlog") return canStart ? "start" : null;
  if (stage === "review" && pr) return "pull_request";
  if (stage === "working" && checkout) return "workspace";
  return null;
}

function card(id: string, workspace: Workspace, scope: BoardScope, checkout: Checkout | null, task: Task, stage: Stage, rows: BoardRow[], now: number): TaskCard {
  const { shown, more } = shownAgents(rows);
  const branch = checkout?.branch ?? checkout?.label ?? null;
  const sourceLabel = workspace.tasks?.source?.label ?? task.source;
  const local = !workspace.remote_target_id;
  const canStart = task.open && checkout === null && local;
  const pr = checkout?.pull_request ? prChip(checkout.pull_request) : null;
  return {
    id,
    place: place(workspace),
    owner: workspace,
    project: scope === "all" ? workspace.label : null,
    checkout,
    task,
    stage,
    title: task.title,
    chip: chipOf(checkout, stage),
    pr,
    mergedAt: checkout?.pull_request?.merged_at_unix_ms ?? null,
    rows,
    shown,
    more,
    needsYou: rows.some((row) => row.agent.group === "needs_you" || (row.depth === 0 && row.agent.group === "done")),
    canStart,
    first: firstAction(stage, canStart, checkout, pr),
    editable: task.source === "local" && local,
    idHelp: [sourceLabel, scope === "all" ? workspace.label : null, branch].filter(Boolean).join(" · "),
    sourceFailure: sourceFailure(workspace, now),
    blockedBy: [],
    updatedAt: task.updated_at_unix_ms ?? null,
  };
}

/** Names each card's blockers once every project's tasks are known, since a blocker may be another project's task (D-10). */
function nameBlockers(cards: TaskCard[], titles: Map<string, string>) {
  for (const value of cards) {
    value.blockedBy = (value.task.blocked_by ?? []).map((ref) => ({ key: ref.key, label: ref.id ?? titles.get(ref.key) ?? ref.key }));
  }
}

/**
 * The Issues board for one Project or for All projects (D-07). Every card is
 * an issue: one a linked worktree works on stands in that worktree's Git
 * stage, so review is an issue whose open pull request reaches it by the
 * branch's issue link or by a closing reference; the primary checkout or a
 * folder carries its issue only while an agent works there; an open issue no
 * checkout works on is in the backlog, most recently changed first. A
 * worktree with no issue in progress and an open pull request with no issue
 * are no card: each column folds them into one line.
 */
export function buildTasks(projects: readonly BoardProject[], scope: BoardScope, now: number): TasksBoard {
  const cards: TaskCard[] = [];
  const backlog: TaskCard[] = [];
  const worktrees: LooseWorktree[] = [];
  const pullRequests: LoosePullRequest[] = [];
  let overflow = false;
  let reading = false;
  let failure: string | null = null;
  let openIssues = 0;
  const labels = new Set<string>();
  const titles = new Map<string, string>();
  for (const { workspace, agents } of projects) {
    const rowsByCheckout = checkoutAgentRows(workspace, agents);
    for (const task of workspace.tasks?.tasks ?? []) titles.set(task.key, task.title);
    const source = workspace.tasks?.source ?? null;
    if (source) {
      labels.add(source.label);
      reading ||= source.reading;
      failure ??= source.failure ? [scope === "all" ? workspace.label : null, `${source.label} 읽기 실패`, "이유는 로그에"].filter(Boolean).join(" · ") : null;
    }
    const tasks = new Map((workspace.tasks?.tasks ?? []).map((task) => [task.key, task]));
    overflow ||= workspace.tasks?.overflow ?? false;
    const git = workspace.is_git === true;
    const worked = new Set<string>();
    for (const checkout of workspace.checkouts) {
      const rows = rowsByCheckout.get(checkout.id) ?? [];
      const task = checkout.task_key ? (tasks.get(checkout.task_key) ?? null) : null;
      if (task) worked.add(task.key);
      if (!(checkout.is_worktree && git)) {
        if (task && rows.length > 0) cards.push(card(`checkout:${checkout.id}`, workspace, scope, checkout, task, "working", rows, now));
        continue;
      }
      const stage = stageOf(checkout);
      if (task) cards.push(card(`checkout:${checkout.id}`, workspace, scope, checkout, task, stage, rows, now));
      // Another issue the same pull request closes shows that pull request too.
      let closes = false;
      for (const key of checkout.closes_task_keys ?? []) {
        const closed = tasks.get(key);
        if (!closed || worked.has(key)) continue;
        worked.add(key);
        closes = true;
        cards.push(card(`closes:${checkout.id}:${key}`, workspace, scope, checkout, closed, stage, [], now));
      }
      if (task || closes) continue;
      const pr = checkout.pull_request;
      if (stage === "working") worktrees.push({ branch: checkout.branch ?? checkout.label });
      else if (stage === "review" && pr) pullRequests.push({ owner: workspace, number: pr.number });
    }
    for (const task of tasks.values()) {
      if (!task.open) continue;
      openIssues += 1;
      if (worked.has(task.key)) continue;
      backlog.push(card(`task:${workspace.id}:${task.key}`, workspace, scope, null, task, "backlog", [], now));
    }
  }
  backlog.sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));
  const all = [...cards, ...backlog];
  nameBlockers(all, titles);
  return {
    cards: prioritized(all),
    loose: { worktrees, pullRequests },
    overflow,
    source: { reading, failure, openIssues: reading ? null : openIssues, label: labels.size === 1 ? ([...labels][0] ?? null) : null },
  };
}

/** The Issues filter at the facts line's right end (B21): words in the id or title, and the operator's turn only. */
export type IssueFilter = { query: string; turn: boolean };

export const NO_FILTER: IssueFilter = { query: "", turn: false };

export function filterActive(filter: IssueFilter): boolean {
  return filter.turn || filter.query.trim() !== "";
}

/** The board with only the cards the filter keeps; the lines of work with no issue are not issues and stay. */
export function filterBoard(board: TasksBoard, filter: IssueFilter): TasksBoard {
  if (!filterActive(filter)) return board;
  const words = filter.query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const keeps = (value: TaskCard) =>
    (!filter.turn || value.needsYou) && words.every((word) => value.title.toLowerCase().includes(word) || (value.task.id ?? "").toLowerCase().includes(word));
  return { ...board, cards: board.cards.filter(keeps) };
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
 * the lock line. A cycle, which a source
 * should not allow, drops the arrow that closes it rather than looping.
 */
export function buildDependencies(board: TasksBoard): DependencyGraph {
  const byKey = new Map<string, TaskCard>();
  for (const value of board.cards) if (!byKey.has(value.task.key)) byKey.set(value.task.key, value);
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

/** A day the way an issue states it, by this machine's calendar: `9월 27일`. */
export function issueDate(unixMs: number): string {
  const date = new Date(unixMs);
  return `${date.getMonth() + 1}월 ${date.getDate()}일`;
}

/** A size the way the Overview writes it: `812 MB`, `1.4 GB`, binary units. */
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

// --- pull requests (PRD overview-lenses-prs) -------------------------------------

/** Whose move a pull request waits on (D-11, D-32): the operator's, an agent fixing it, nobody though it is blocked, or merged. */
export type PrGroup = "turn" | "fixing" | "blocked" | "merged";

const PR_GROUPS: readonly { group: PrGroup; label: string }[] = [
  { group: "turn", label: "내 차례" },
  { group: "fixing", label: "에이전트가 고치는 중" },
  { group: "blocked", label: "CI 실패 · 맡은 에이전트 없음" },
  { group: "merged", label: "최근 머지" },
];

/** An agent at work, a parent waiting on its working children included (status-model's activity axis, PRD Risks). */
function isWorking(agent: AgentRow): boolean {
  return agent.group === "working" || agent.waiting_on_descendants === true;
}

/** The issue a pull request works on: the task when the source lists it, else the reference its body closes. */
export type PrIssue = { key: string; label: string; url: string | null; task: Task | null };

/** What `정리` on a merged row removes (B20): the worktree with its folder, or only its record. */
export type PrCleanup = "worktree" | "record";

/** One pull request row of the PRs tab (B3-B8). */
export type PrRow = {
  number: number;
  title: string;
  url: string;
  tone: PrTone;
  group: PrGroup;
  pr: PullRequest;
  branch: string;
  /** The checkout on its branch, a worktree whose folder is gone included. */
  checkout: Checkout | null;
  issue: PrIssue | null;
  /** The checkout's agents, the ones that need the operator first, for the marks (B4). */
  agents: AgentRow[];
  /** The checkout's agents with their ancestors, root first, for the unfolded row (B5). */
  lineage: BoardRow[];
  /** An agent there finished and was not looked at yet: the yellow `확인` (D-48). */
  needsLook: boolean;
  checks: "passing" | "failed" | "pending" | null;
  review: "review_required" | "changes_requested" | "approved" | null;
  /** When it last changed, or merged, for the time column. */
  at: number | null;
  /** `▷ 맡기기` stands in the hover slot (B6, B15). */
  delegate: boolean;
  /** The issue cell offers 이슈 잇기 (B7, B9): an open pull request with no issue, where its source can link one. */
  linkable: boolean;
  cleanup: PrCleanup | null;
};

export type PrBoard = {
  groups: { group: PrGroup; label: string; rows: PrRow[] }[];
  /** Open pull requests, or null until GitHub has answered (B22). */
  open: number | null;
  /** `gh` has not answered yet for this project (B22). */
  reading: boolean;
  /** Why the last read failed, with the value's age; the reason is in the log (B22). */
  failure: string | null;
};

const PR_ATTENTION = (agent: AgentRow) => (agent.group === "needs_you" ? 0 : agent.group === "done" ? 1 : isWorking(agent) ? 2 : 3);

/** The ancestors of the checkout's agents, root first, then the checkout's own rows (B5). */
function prLineage(rows: BoardRow[], agents: readonly AgentRow[]): BoardRow[] {
  const byPane = new Map(agents.map((agent) => [agent.pane_id, agent]));
  const shown = new Set(rows.map((row) => row.agent.pane_id));
  const result: BoardRow[] = [];
  for (const row of rows) {
    if (row.depth !== 0) continue;
    const ancestors: AgentRow[] = [];
    let parent = row.agent.lineage_parent_pane_id ? byPane.get(row.agent.lineage_parent_pane_id) : undefined;
    while (parent && !shown.has(parent.pane_id) && ancestors.length < 8) {
      ancestors.unshift(parent);
      parent = parent.lineage_parent_pane_id ? byPane.get(parent.lineage_parent_pane_id) : undefined;
    }
    ancestors.forEach((agent, depth) => {
      shown.add(agent.pane_id);
      result.push({ agent, depth });
    });
    const start = rows.indexOf(row);
    for (let index = start; index < rows.length && (index === start || (rows[index]?.depth ?? 0) > 0); index += 1) {
      const next = rows[index] as BoardRow;
      result.push({ agent: next.agent, depth: next.depth + ancestors.length });
    }
  }
  return result;
}

/** The issue of a pull request: its checkout's linked task, else the first issue its body closes. */
function prIssue(workspace: Workspace, checkout: Checkout | null, pr: PullRequest, tasks: Map<string, Task>): PrIssue | null {
  const linked = checkout?.task_key ? tasks.get(checkout.task_key) : undefined;
  if (linked) return { key: linked.key, label: linked.id ?? linked.title, url: linked.url, task: linked };
  const repository = workspace.home_issues?.repository ?? null;
  for (const reference of pr.closing_issues ?? []) {
    const key = `github:${reference.repository}#${reference.number}`;
    const task = tasks.get(key) ?? null;
    const label = task?.id ?? (reference.repository === repository ? `#${reference.number}` : `${reference.repository}#${reference.number}`);
    return { key, label, url: task?.url ?? `https://github.com/${reference.repository}/issues/${reference.number}`, task };
  }
  return null;
}

/**
 * The PRs tab (D-11, D-32, D-52) for one project: its pull requests grouped
 * by whose move it is. A merged one is `최근 머지`, newest merge first; an
 * open one whose branch's checkout has a working agent is `에이전트가 고치는
 * 중`; otherwise failed checks or a change request make it `CI 실패 · 맡은
 * 에이전트 없음`, and every other open one (asking for review, approved, an
 * agent finished there, a draft) is the operator's. A group with no row is
 * not drawn. The core sends only the pull requests the tab shows.
 */
export function buildPullRequests(project: BoardProject, now: number): PrBoard {
  const { workspace, agents } = project;
  const tasks = new Map((workspace.tasks?.tasks ?? []).map((task) => [task.key, task]));
  const rowsByCheckout = checkoutAgentRows(workspace, agents);
  const local = workspace.tasks?.source?.kind === "local";
  const rows: PrRow[] = [];
  for (const pr of workspace.pull_requests ?? []) {
    const branch = pr.head_branch ?? "";
    const checkout = workspace.checkouts.find((row) => row.branch === branch && (row.is_worktree || row.exists)) ?? null;
    const boardRows = checkout ? (rowsByCheckout.get(checkout.id) ?? []) : [];
    const panes = new Set(checkout?.tabs.flatMap((tab) => tab.panes.map((pane) => pane.id)) ?? []);
    const agentsHere = agents.filter((agent) => panes.has(agent.pane_id)).sort((a, b) => PR_ATTENTION(a) - PR_ATTENTION(b));
    const chip = prChip(pr);
    const merged = pr.badge === "merged";
    const checks = chip.checks;
    const review = merged ? null : pr.review;
    const group: PrGroup = merged
      ? "merged"
      : agentsHere.some(isWorking)
        ? "fixing"
        : checks === "failed" || review === "changes_requested"
          ? "blocked"
          : "turn";
    const issue = prIssue(workspace, checkout, pr, tasks);
    const folderGone = checkout !== null && (!checkout.exists || checkout.worktree?.missing === true);
    const worktree = checkout?.is_worktree === true && checkout.is_primary !== true;
    rows.push({
      number: pr.number,
      title: pr.title,
      url: pr.url,
      tone: chip.tone,
      group,
      pr,
      branch,
      checkout,
      issue,
      agents: agentsHere,
      lineage: checkout ? prLineage(boardRows, agents) : [],
      needsLook: agentsHere.some((agent) => agent.group === "done"),
      checks,
      review,
      at: merged ? (pr.merged_at_unix_ms ?? null) : (pr.updated_at_unix_ms ?? null),
      delegate: group === "blocked",
      linkable: !merged && issue === null && (!local || (worktree && !folderGone)),
      cleanup: merged && worktree ? (folderGone ? "record" : "worktree") : null,
    });
  }
  const byRecent = (a: PrRow, b: PrRow) => (b.at ?? 0) - (a.at ?? 0) || b.number - a.number;
  const groups = PR_GROUPS.map(({ group, label }) => ({ group, label, rows: rows.filter((row) => row.group === group).sort(byRecent) })).filter((entry) => entry.rows.length > 0);
  const status = workspace.checkouts.find((checkout) => checkout.github)?.github ?? null;
  // A repository with no GitHub remote has no pull requests, which is an answer, not a failure (design 13).
  const noRemote = status?.failure_category === "no GitHub remote";
  const answered = status?.last_success_at_unix_ms != null || noRemote;
  const failed = !noRemote && (status?.stale === true || (status?.unavailable_reason != null && !status.available));
  const age = status?.last_success_at_unix_ms != null ? Math.max(0, Math.floor((now - status.last_success_at_unix_ms) / 60_000)) : null;
  return {
    groups,
    open: answered ? rows.filter((row) => row.group !== "merged").length : null,
    reading: !answered && !failed,
    failure: failed ? ["GitHub 읽기 실패", age === null ? "읽은 값 없음" : `${age}분 전 값`, "이유는 로그에"].join(" · ") : null,
  };
}
