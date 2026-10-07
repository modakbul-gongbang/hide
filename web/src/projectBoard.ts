import { scopeOccurrences, scopeRows, type AgentTreeScope, type AgentScope } from "./agentScope";
// The Overview's Issues view (PRD task-agents-views, reworked issue-first
// on 2026-09-28 and issues-only by PRD overview-lenses-issues) as pure
// functions over the snapshot, for one Project or for All projects. Every
// card is an issue: the checkout and agents working on it and the pull
// request they opened ride on it, and work with no issue folds into one line
// per column (D-07). Git decides the stage; agents and the issue's own state
// never move it. Every value drawn is one the snapshot carries, and a count
// the source has not answered for is left out rather than drawn as zero
// (design 10).

import type { TFunction } from "i18next";
import { formatDateTime } from "./i18n/format";
import type { MessageKey } from "./i18n/catalogs";
import { requireInterfaceLanguage } from "./i18n/locale";
import type { AgentRow, Checkout, IssueLabel, PullRequest, Task, TaskSubIssue, Workspace } from "./snapshot";

export type Stage = "backlog" | "working" | "review" | "done";

export const STAGES: readonly { stage: Stage; labelKey: MessageKey }[] = [
  { stage: "backlog", labelKey: "board.stage.backlog" },
  { stage: "working", labelKey: "board.stage.working" },
  { stage: "review", labelKey: "board.stage.review" },
  { stage: "done", labelKey: "board.stage.done" },
];

/**
 * A source that could not be read, as data: the words are written where it
 * is drawn, in the interface language. `value` says how the last value
 * stands: left out of the line, `last` known without an age, `none` ever
 * read, or `minutes` old. The reason is in the diagnostic log (design 13).
 */
export type ReadFailure = {
  /** The Project's name, only where the scope holds several. */
  project: string | null;
  /** The source that failed: `GitHub`, `Local`. */
  source: string;
  value: "omitted" | "last" | "none" | { minutes: number };
};

export function readFailureText(failure: ReadFailure, t: TFunction<"translation">): string {
  const value = failure.value;
  const stands = value === "omitted" ? null : value === "last" ? t("board.lastValue") : value === "none" ? t("board.noValue") : t("board.valueMinutes", { count: value.minutes });
  return [failure.project, t("board.sourceReadFailed", { source: failure.source }), stands, t("board.reasonLogged")].filter(Boolean).join(" · ");
}

/** A checkout's stage: only an issue nobody works on is in the backlog. */
export type GitStage = Exclude<Stage, "backlog">;

/** A checkout's Git stage: its work landed or its pull request merged, then an open pull request, else work in progress. Agents and the issue's state never move it. */
export function stageOf(checkout: Checkout): GitStage {
  const pr = checkout.pull_request;
  if (checkout.landed === true || pr?.badge === "merged") return "done";
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

/** What a card's hover slot offers first (B6): Start on the backlog, the Workspace in progress, the pull request in review. */
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
  turnKind: "question" | "review" | null;
  /** An open backlog issue of a project on this Mac: Start opens the Start dialog. */
  canStart: boolean;
  first: FirstAction;
  /** A Local issue: its title and body are edited in its panel (D-41). */
  editable: boolean;
  /** The id's hint: where the issue comes from and the branch working on it. */
  idHelp: string;
  /** Why the card's source could not be read and how old its value is (B17). */
  sourceFailure: ReadFailure | null;
  /** The open tasks this one waits on, the lock line: each by its id, else its title when the scope has it. */
  blockedBy: Blocker[];
  /** When the issue last changed, or null; the backlog's order. */
  updatedAt: number | null;
  /** GitHub's progress over the issue's sub-issues and each one's closing pull request; null for an issue with none. */
  subIssues: CardSubIssues | null;
};

/** A sub-issue of a card with the pull request whose body closes it, when the project has one. */
export type CardSubIssue = TaskSubIssue & { pr: PrChip | null };

export type CardSubIssues = { total: number; completed: number; items: CardSubIssue[] };

/** A worktree with no issue, one of `N worktrees without an issue` under In progress (B4). */
export type LooseWorktree = { branch: string };

/** An open pull request with no issue, one of `N PRs without an issue` under Review (B4). */
export type LoosePullRequest = { owner: Workspace; number: number };

export type Blocker = { key: string; label: string };

/** A Project the board reads: its catalog row, every agent its device reported, and the device's name when it is not this machine. */
export type BoardProject = { workspace: Workspace; agents: AgentRow[]; device: string | null };

export type BoardScope = "project" | "all";

/** How the scope's issue sources stand, for the backlog's header and the facts line. */
export type SourceState = {
  /** A source is still answering its first read. */
  reading: boolean;
  /** That a source could not be read (the first one), for a mark beside Backlog; the reason is in the diagnostic log (design 13). */
  failure: ReadFailure | null;
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

/** The representatives selected by the core for this tree. */
export function shownAgents(tree: AgentTreeScope, agents: AgentRow[]): { shown: AgentRow[]; more: number } {
  const refs = new Map(tree.rows.map(row => [row.pane_id, row]));
  return { shown: scopeOccurrences(tree.shown.map(id => {
    const row = refs.get(id);
    if (!row) throw new Error(`Missing core tree representative: ${id}`);
    return row;
  }), agents), more: tree.more };
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
export function checkoutAgentRows(workspace: Workspace, agents: AgentRow[], context: "device" | "global" | "visible" = "device"): Map<string, BoardRow[]> {
  return new Map(workspace.checkouts.map((checkout) => {
    const tree = context === "device" ? checkout.agent_scope.tree : checkout.agent_scope.global_tree;
    const rows = context === "visible" ? tree.visible_rows : tree.rows;
    return [checkout.id, scopeOccurrences(rows, agents).map((agent, index) => ({ agent, depth: rows[index]!.depth }))];
  }));
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
function sourceFailure(workspace: Workspace, now: number): ReadFailure | null {
  const source = workspace.tasks?.source;
  if (!source?.failure) return null;
  const age = source.last_read_at_unix_ms == null ? null : Math.max(0, Math.floor((now - source.last_read_at_unix_ms) / 60_000));
  return { project: null, source: source.label, value: age === null ? "last" : { minutes: age } };
}

function firstAction(stage: Stage, canStart: boolean, checkout: Checkout | null, pr: PrChip | null): FirstAction {
  if (stage === "backlog") return canStart ? "start" : null;
  if (stage === "review" && pr) return "pull_request";
  if (stage === "working" && checkout) return "workspace";
  return null;
}

function card(id: string, workspace: Workspace, scope: BoardScope, checkout: Checkout | null, task: Task, stage: Stage, rows: BoardRow[], now: number): TaskCard {
  const tree = checkout?.agent_scope.tree;
  const shown = tree ? scopeRows(tree.shown, rows.map((row) => row.agent)) : [];
  const more = tree?.more ?? 0;
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
    needsYou: tree?.needs_you ?? false,
    turnKind: tree?.turn_kind ?? null,
    canStart,
    first: firstAction(stage, canStart, checkout, pr),
    editable: task.source === "local" && local,
    idHelp: [sourceLabel, scope === "all" ? workspace.label : null, branch].filter(Boolean).join(" · "),
    sourceFailure: sourceFailure(workspace, now),
    blockedBy: [],
    updatedAt: task.updated_at_unix_ms ?? null,
    subIssues: subIssuesOf(workspace, task),
  };
}

/** A sub-issue's pull request is the one whose body closes it (`closing_issues`); a mention that does not close it is not one. */
function subIssuesOf(workspace: Workspace, task: Task): CardSubIssues | null {
  const subIssues = task.sub_issues;
  if (!subIssues) return null;
  const pulls = workspace.pull_requests ?? [];
  const closing = (key: string) => {
    const closes = pulls.filter((pr) => (pr.closing_issues ?? []).some((reference) => `github:${reference.repository}#${reference.number}` === key));
    return closes.find((pr) => pr.badge !== "closed") ?? closes[0];
  };
  return {
    total: subIssues.total,
    completed: subIssues.completed,
    items: subIssues.items.map((item) => {
      const pr = closing(item.key);
      return { ...item, pr: pr ? prChip(pr) : null };
    }),
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
  let failure: ReadFailure | null = null;
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
      failure ??= source.failure ? { project: scope === "all" ? workspace.label : null, source: source.label, value: "omitted" } : null;
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

/**
 * The Issues filter at the facts line's right end (B21): words in the id or
 * title, the operator's turn only, and labels by name, of which a card has to
 * carry any one.
 */
export type IssueFilter = { query: string; turn: boolean; labels: readonly string[] };

export const NO_FILTER: IssueFilter = { query: "", turn: false, labels: [] };

export function filterActive(filter: IssueFilter): boolean {
  return filter.turn || filter.query.trim() !== "" || filter.labels.length > 0;
}

/** The board with only the cards the filter keeps; the lines of work with no issue are not issues and stay. */
export function filterBoard(board: TasksBoard, filter: IssueFilter): TasksBoard {
  if (!filterActive(filter)) return board;
  const words = filter.query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const picked = new Set(filter.labels);
  const keeps = (value: TaskCard) =>
    (!filter.turn || value.needsYou) &&
    (picked.size === 0 || (value.task.labels ?? []).some((label) => picked.has(label.name))) &&
    words.every((word) => value.title.toLowerCase().includes(word) || (value.task.id ?? "").toLowerCase().includes(word));
  return { ...board, cards: board.cards.filter(keeps) };
}

/** A label the filter offers: its name, a card's colour for it, and how many cards carry it. */
export type BoardLabel = { label: IssueLabel; count: number };

/**
 * The labels the board's cards carry, the filter's choices (B21), in name
 * order. Labels are one by name across projects, coloured as the first card
 * that carries one has it. A picked label no card carries any more stays
 * offered with no cards, so it can still be taken off.
 */
export function boardLabels(board: TasksBoard, picked: readonly string[]): BoardLabel[] {
  const byName = new Map<string, BoardLabel>();
  for (const value of board.cards) {
    for (const label of value.task.labels ?? []) {
      const known = byName.get(label.name);
      if (known) known.count += 1;
      else byName.set(label.name, { label, count: 1 });
    }
  }
  for (const name of picked) if (!byName.has(name)) byName.set(name, { label: { name, color: null }, count: 0 });
  return [...byName.values()].sort((a, b) => a.label.name.localeCompare(b.label.name));
}

/** The cards of one Tasks column. */
export function stageCards(board: TasksBoard, stage: Stage): TaskCard[] {
  return board.cards.filter((value) => value.stage === stage);
}

/** One arrow of the Dependencies mode: from the blocker's card to the card it blocks. */
export type DependencyEdge = { from: string; to: string };

/** The tasks that wait on or block another in scope, in columns left to right by how many blockers precede them, and the tasks with no relation in scope, gathered below the graph (D-09). */
export type DependencyGraph = LayeredGraph<TaskCard>;

/**
 * The Dependencies mode (D-09, D-10): the Board's task cards, laid out left
 * to right (`layerDependencies`). Keys name tasks across projects, so a
 * blocker in another project of the scope is an arrow too; one outside the
 * scope is only the lock line.
 */
export function buildDependencies(board: TasksBoard): DependencyGraph {
  const byKey = new Map<string, TaskCard>();
  for (const value of board.cards) if (!byKey.has(value.task.key)) byKey.set(value.task.key, value);
  return layerDependencies([...byKey.values()], (value) => value.id, (value) =>
    value.blockedBy.flatMap((blocker) => {
      const from = byKey.get(blocker.key);
      return from && from !== value ? [from] : [];
    }));
}

/** Nodes laid out by what they wait on: the layers left to right, the arrows between them, and the nodes with no relation. */
export type LayeredGraph<T> = { layers: T[][]; edges: DependencyEdge[]; unrelated: T[] };

/**
 * The layered dependency layout the Issues view and the Factory graph share:
 * a node's column is the longest chain of blockers before it, and each
 * column is ordered by the mean row of the blockers it hangs from, so arrows
 * mostly run straight. A cycle, which a source should not allow, drops the
 * arrow that closes it rather than looping.
 */
export function layerDependencies<T>(nodes: readonly T[], idOf: (node: T) => string, blockersOf: (node: T) => readonly T[]): LayeredGraph<T> {
  const blockers = new Map<string, readonly T[]>();
  const edges: DependencyEdge[] = [];
  const related = new Set<string>();
  for (const value of nodes) {
    const before = blockersOf(value);
    blockers.set(idOf(value), before);
    for (const from of before) {
      edges.push({ from: idOf(from), to: idOf(value) });
      related.add(idOf(from));
      related.add(idOf(value));
    }
  }
  const depth = new Map<string, number>();
  const visiting = new Set<string>();
  const depthOf = (value: T): number => {
    const id = idOf(value);
    const known = depth.get(id);
    if (known !== undefined) return known;
    if (visiting.has(id)) return -1;
    visiting.add(id);
    const found = Math.max(-1, ...(blockers.get(id) ?? []).map(depthOf)) + 1;
    visiting.delete(id);
    depth.set(id, found);
    return found;
  };
  const layers: T[][] = [];
  for (const value of nodes) {
    if (!related.has(idOf(value))) continue;
    const layer = depthOf(value);
    (layers[layer] ??= []).push(value);
  }
  const row = new Map<string, number>();
  const dense = layers.filter((layer) => layer !== undefined);
  for (const layer of dense) {
    const weight = (value: T) => {
      const rows = (blockers.get(idOf(value)) ?? []).flatMap((from) => (row.has(idOf(from)) ? [row.get(idOf(from)) as number] : []));
      return rows.length > 0 ? rows.reduce((sum, at) => sum + at, 0) / rows.length : Number.POSITIVE_INFINITY;
    };
    const ordered = layer.map((value, index) => ({ value, index, weight: weight(value) })).sort((a, b) => a.weight - b.weight || a.index - b.index);
    layer.splice(0, layer.length, ...ordered.map(({ value }) => value));
    layer.forEach((value, index) => row.set(idOf(value), index));
  }
  const forward = edges.filter((edge) => (depth.get(edge.from) ?? 0) < (depth.get(edge.to) ?? 0));
  return { layers: dense, edges: forward, unrelated: nodes.filter((value) => !related.has(idOf(value))) };
}

/**
 * The arrows left once every arrow implied by a longer path is dropped
 * (PRD software-factory-ui D-08): with A→B→C, A→C is not drawn. The data
 * keeps every edge; this is a drawing step, used by the Factory graph only.
 * An edge on a cycle is kept, since no other path stands for it.
 */
export function transitiveReduction(edges: readonly DependencyEdge[]): DependencyEdge[] {
  const next = new Map<string, string[]>();
  for (const edge of edges) next.set(edge.from, [...(next.get(edge.from) ?? []), edge.to]);
  // Whether `to` is reachable from `from` by a path of two or more arrows.
  const longer = (from: string, to: string) => {
    const seen = new Set<string>();
    const stack = (next.get(from) ?? []).filter((step) => step !== to);
    while (stack.length > 0) {
      const at = stack.pop()!;
      if (at === to) return true;
      if (seen.has(at) || at === from) continue;
      seen.add(at);
      stack.push(...(next.get(at) ?? []));
    }
    return false;
  };
  const unique = edges.filter((edge, index) => edges.findIndex((other) => other.from === edge.from && other.to === edge.to) === index);
  return unique.filter((edge) => !longer(edge.from, edge.to));
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
   * there is no number: never asked, or no checkout could be read. While one
   * checkout could not be, it is the subtotal of the others (`entranceBytes`
   * says which it is).
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
    // A checkout that could not be measured leaves no total, and the entrance still states the subtotal of those that were.
    disk: workspace.disk?.total_bytes ?? (workspace.disk?.measuring ? "measuring" : (workspace.disk?.confirmed_bytes ?? null)),
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
    if (!workspace.is_git || workspace.checkouts.some((checkout) => checkout.github?.failure_category === "no_github_remote")) continue;
    openPullRequests = openPullRequests === null || stats.openPullRequests === null ? null : openPullRequests + stats.openPullRequests;
  }
  return { projects: workspaces.length, openPullRequests: known ? openPullRequests : null, merged: known ? merged : null };
}

/** A day the way an issue states it, by this machine's calendar: `Sep 27`, `9월 27일`. */
export function issueDate(unixMs: number, language: string): string {
  return formatDateTime(requireInterfaceLanguage(language), unixMs, { month: "short", day: "numeric" });
}

// --- pull requests (PRD overview-lenses-prs) -------------------------------------

/** Whose move a pull request waits on (D-11, D-32): the operator's, an agent fixing it, nobody though it is blocked, or merged. */
export type PrGroup = "turn" | "fixing" | "blocked" | "merged";

export const PR_GROUP_LABEL: Record<PrGroup, MessageKey> = {
  turn: "board.prGroup.turn",
  fixing: "board.prGroup.fixing",
  blocked: "board.prGroup.blocked",
  merged: "board.prGroup.merged",
};

/** The issue a pull request works on: the task when the source lists it, else the reference its body closes. */
export type PrIssue = { key: string; label: string; url: string | null; task: Task | null };

/** What `Clean up` on a merged row removes (B20): the worktree with its folder, or only its record. */
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
  /** The checkout's agents and the agent whose session made it (overview-request-view D-45), the ones that need the operator first, for the marks (B4). */
  agents: AgentRow[];
  /** The checkout's agents with their ancestors, root first, for the unfolded row (B5). */
  lineage: BoardRow[];
  /** An agent there finished and was not looked at yet: the yellow `Review` (D-48). */
  needsLook: boolean;
  checks: "passing" | "failed" | "pending" | null;
  review: "review_required" | "changes_requested" | "approved" | null;
  /** When it last changed, or merged, for the time column. */
  at: number | null;
  /** `▷ Assign` stands in the hover slot (B6, B15). */
  delegate: boolean;
  /** The issue cell offers Link issue (B7, B9): an open pull request with no issue, where its source can link one. */
  linkable: boolean;
  cleanup: PrCleanup | null;
};

export type PrBoard = {
  counts: AgentScope["prs"]["counts"];
  groups: { group: PrGroup; rows: PrRow[] }[];
  /** Open pull requests, or null until GitHub has answered (B22). */
  open: number | null;
  /** `gh` has not answered yet for this project (B22). */
  reading: boolean;
  /** That the last read failed, with the value's age; the reason is in the log (B22). */
  failure: ReadFailure | null;
};

/**
 * The PRs tab (D-11, D-32, D-52) for one project: its pull requests grouped
 * by whose move it is. A merged one is `Recently merged`, newest merge first; an
 * open one whose branch's checkout has a working agent is `Agent is fixing`;
 * otherwise failed checks or a change request make it `CI failed · no agent
 * assigned`, and every other open one (asking for review, approved, an
 * agent finished there, a draft) is the operator's. A group with no row is
 * not drawn. The core sends only the pull requests the tab shows.
 */
export function buildPullRequests(project: BoardProject, now: number): PrBoard {
  const { workspace, agents } = project;
  const tasks = new Map((workspace.tasks?.tasks ?? []).map((task) => [task.key, task]));
  const board = workspace.agent_scope.prs;
  const local = workspace.tasks?.source?.kind === "local";
  const rows: PrRow[] = [];
  for (const pr of workspace.pull_requests ?? []) {
    const branch = pr.head_branch ?? "";
    // The checkout the core connected it to (a settled pull request only at the
    // exact commit), never a branch of the same name: a branch used again for
    // new work is not the merged pull request's worktree to clean up.
    const state = board.rows.find((row) => row.number === pr.number);
    if (!state) throw new Error(`Missing core PR row: ${pr.number}`);
    const checkout = state.checkout_id === null ? null : workspace.checkouts.find((c) => c.id === state.checkout_id);
    if (checkout === undefined) throw new Error(`Missing PR checkout: ${state.checkout_id}`);
    const agentsHere = scopeOccurrences(state.agents, agents);
    const chip = prChip(pr);
    const merged = pr.badge === "merged";
    const checks = chip.checks;
    const review = merged ? null : pr.review;
    const group = state.group;
    const issue = state.issue ? { ...state.issue, task: state.issue.task_key ? tasks.get(state.issue.task_key) ?? null : null } : null;
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
      lineage: scopeOccurrences(state.lineage, agents).map((agent, index) => ({ agent, depth: state.lineage[index]!.depth })),
      needsLook: state.needs_look,
      checks,
      review,
      at: merged ? (pr.merged_at_unix_ms ?? null) : (pr.updated_at_unix_ms ?? null),
      delegate: group === "blocked",
      linkable: !merged && issue === null && (!local || (worktree && !folderGone)),
      cleanup: merged && worktree ? (folderGone ? "record" : "worktree") : null,
    });
  }
  const byNumber = new Map(rows.map((row) => [row.number, row]));
  const groups = board.groups.map((group) => ({ group: group.group, rows: group.numbers.map((number) => {
    const row = byNumber.get(number);
    if (!row) throw new Error(`Missing grouped PR: ${number}`);
    return row;
  }) }));
  const status = workspace.checkouts.find((checkout) => checkout.github)?.github ?? null;
  // A repository with no GitHub remote has no pull requests, which is an answer, not a failure (design 13).
  const noRemote = status?.failure_category === "no_github_remote";
  const answered = status?.last_success_at_unix_ms != null || noRemote;
  const failed = !noRemote && (status?.stale === true || (status?.unavailable_reason != null && !status.available));
  const age = status?.last_success_at_unix_ms != null ? Math.max(0, Math.floor((now - status.last_success_at_unix_ms) / 60_000)) : null;
  return {
    groups,
    counts: board.counts,
    open: answered ? board.open : null,
    reading: !answered && !failed,
    failure: failed ? { project: null, source: "GitHub", value: age === null ? "none" : { minutes: age } } : null,
  };
}
