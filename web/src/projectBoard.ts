// A Project's Overview board (PRD web-project-overview D-02, D-03): the
// Swift Project Home rules (`ProjectHomePresentation.swift`) as one pure
// function over the snapshot, so both shells put a checkout in the same
// column. Every value drawn is one the snapshot carries; a count GitHub has
// not answered for is left out rather than drawn as zero (design 10).

import { projectAgents } from "./navigation";
import type { AgentRow, Checkout, GithubStatus, IssueLink, Workspace } from "./snapshot";

export type Stage = "ready" | "working" | "review" | "merged";

export const STAGES: readonly { stage: Stage; label: string }[] = [
  { stage: "ready", label: "준비" },
  { stage: "working", label: "작업 중" },
  { stage: "review", label: "리뷰" },
  { stage: "merged", label: "머지됨" },
];

/** A checkout's Git stage: merged, then an open pull request, then local work, else ready. Agents and issue status never move it. */
export function stageOf(checkout: Checkout): Stage {
  const pr = checkout.pull_request;
  if (checkout.worktree?.merged === true || pr?.badge === "merged") return "merged";
  if (pr && pr.badge !== "closed") return "review";
  if ((checkout.changed_file_count ?? 0) > 0 || (checkout.ahead ?? 0) > 0) return "working";
  return "ready";
}

const PROJECT_STATUS: Record<Stage, readonly string[]> = {
  ready: ["준비", "todo", "to do", "backlog", "ready"],
  working: ["작업 중", "working", "in progress"],
  review: ["리뷰", "review", "in review"],
  merged: ["머지됨", "done", "merged", "complete", "completed"],
};

/** Whether a GitHub Project status names the same stage the checkout's Git state does. */
export function stageMatches(stage: Stage, projectStatus: string): boolean {
  return PROJECT_STATUS[stage].includes(projectStatus.trim().toLowerCase());
}

export type BoardRow = {
  agent: AgentRow;
  /** 0 for a root, one more per delegation step. */
  depth: number;
};

/** The one delivery fact a card's footer states. */
export type Delivery = {
  label: string;
  /** The pull request's CI, only for a card in review whose checks were read. */
  checks: "passing" | "failed" | "pending" | null;
};

export type BoardCard = {
  id: string;
  checkout: Checkout | null;
  rows: BoardRow[];
  issue: IssueLink | null;
  stage: Stage | null;
  /** An open issue no checkout is linked to, shown in 준비. */
  backlog: boolean;
  needsYou: boolean;
  /** A row reports an error; the halo is drawn in danger instead of warning. */
  error: boolean;
  title: string | null;
  delivery: Delivery | null;
  /** The issue's Project status when it names another stage than Git's. */
  mismatch: string | null;
  mismatchHelp: string | null;
  issueHelp: string | null;
};

export type AgentColumn = "active" | "done" | "seen";

export const AGENT_COLUMNS: readonly { column: AgentColumn; label: string; groups: readonly string[] }[] = [
  { column: "active", label: "진행 중", groups: ["working", "needs_you"] },
  { column: "done", label: "내 확인 대기", groups: ["done"] },
  { column: "seen", label: "끝", groups: ["seen"] },
];

export type BoardStats = {
  worktrees: number;
  /** Open pull requests, or null until GitHub has answered for this project. */
  openPullRequests: number | null;
  /** The primary checkout's branch and how far origin is ahead of it, only when it is. */
  behind: { branch: string; count: number } | null;
  /** Linked worktrees whose work is merged, the ones the Merged column lists for removal. */
  merged: number;
  /**
   * Allocated disk in bytes, `measuring` while the core walks it, or null when
   * there is no number: never asked, or a part could not be read.
   */
  disk: number | "measuring" | null;
};

/**
 * What the Overview draws, in precedence order (D-06): no agent at all is the
 * empty state whatever the project is; a folder with agents has only the ad
 * hoc strip; otherwise the whole board.
 */
export type BoardState = "empty" | "adhoc" | "board";

export type Board = {
  state: BoardState;
  tasks: BoardCard[];
  adHoc: BoardCard[];
  agents: BoardCard[];
  /** The core capped the issue list, so 준비 may hold more than it shows. */
  overflow: boolean;
  stats: BoardStats;
};

function delivery(checkout: Checkout, stage: Stage): Delivery {
  switch (stage) {
    case "ready":
      return { label: "변경 없음", checks: null };
    case "working": {
      const changed = checkout.changed_file_count ?? 0;
      const ahead = checkout.ahead ?? 0;
      const parts = [changed > 0 ? `변경 ${changed}` : null, ahead > 0 ? `↑${ahead} 커밋` : null].filter(Boolean);
      return { label: parts.join(" · "), checks: null };
    }
    case "review": {
      const pr = checkout.pull_request;
      const checks = pr?.checks === "passing" || pr?.checks === "failed" || pr?.checks === "pending" ? pr.checks : null;
      return { label: pr ? `PR #${pr.number}` : "PR", checks };
    }
    case "merged":
      return { label: "머지됨", checks: null };
  }
}

const STAGE_LABEL = Object.fromEntries(STAGES.map(({ stage, label }) => [stage, label])) as Record<Stage, string>;

function issueHelp(link: IssueLink | null, github: GithubStatus | null, now: number): string | null {
  if (!link) return null;
  const { issue } = link;
  const lines = [`${issue.reference.repository}#${issue.reference.number} · ${issue.state === "CLOSED" ? "닫힘" : "열림"}`, issue.title];
  if (issue.project_status) lines.push(`Project: ${issue.project_status}`);
  lines.push(link.source);
  // GitHub's age is said only here, never in a banner (B11).
  if (github && (github.loading || github.stale) && github.last_success_at_unix_ms != null) {
    lines.push(`GitHub: 마지막 성공 ${Math.max(0, Math.floor((now - github.last_success_at_unix_ms) / 60_000))}분 전`);
  }
  return lines.join(" · ");
}

function card(
  id: string,
  checkout: Checkout | null,
  rows: BoardRow[],
  issue: IssueLink | null,
  stage: Stage | null,
  backlog: boolean,
  github: GithubStatus | null,
  now: number,
): BoardCard {
  const status = issue?.issue.project_status ?? null;
  const mismatch = status && stage && !stageMatches(stage, status) ? status : null;
  const fact = checkout && stage && !backlog ? delivery(checkout, stage) : null;
  return {
    id,
    checkout,
    rows,
    issue,
    stage,
    backlog,
    needsYou: rows.some((row) => row.agent.group === "needs_you"),
    error: rows.some((row) => row.agent.demand === "error"),
    title: issue?.issue.title ?? checkout?.purpose?.text ?? null,
    delivery: fact,
    mismatch,
    mismatchHelp: mismatch && stage ? `Project: ${mismatch} · git: ${stage === "ready" || !fact?.label ? STAGE_LABEL[stage] : fact.label}${stage === "review" ? " 열림" : ""}` : null,
    issueHelp: issueHelp(issue, checkout?.github ?? github, now),
  };
}

/** Needs-you cards first, each group in its original order; nothing else reorders a column. */
function prioritized(cards: BoardCard[]): BoardCard[] {
  return cards
    .map((value, index) => ({ value, index }))
    .sort((a, b) => (a.value.needsYou === b.value.needsYou ? a.index - b.index : a.value.needsYou ? -1 : 1))
    .map(({ value }) => value);
}

/** A Git project's facts line (B2); the All projects scope sums them across projects. */
export function projectStats(workspace: Workspace): BoardStats {
  const answered = workspace.checkouts.some((checkout) => checkout.github?.last_success_at_unix_ms != null);
  const primary = workspace.checkouts.find((checkout) => checkout.worktree?.is_main) ?? workspace.checkouts.find((checkout) => !checkout.is_worktree) ?? null;
  const behind = primary?.worktree?.behind_upstream ?? 0;
  return {
    worktrees: workspace.checkouts.filter((checkout) => checkout.is_worktree).length,
    openPullRequests: answered ? workspace.checkouts.filter((checkout) => checkout.pull_request?.badge === "open" || checkout.pull_request?.badge === "review").length : null,
    behind: primary && behind > 0 ? { branch: primary.branch ?? primary.label, count: behind } : null,
    merged: workspace.checkouts.filter((checkout) => checkout.is_worktree && stageOf(checkout) === "merged").length,
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

/**
 * Which checkout owns each pane, and a lineage walker over `agents`: a list
 * of agents drawn with their whole lineage, root first, whatever the sidebar
 * has folded, so its rows carry no descendant badge; a row's branch chip is
 * the Agents list's rule (`branchChip`), a checkout that differs from its
 * parent's.
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
 * Each checkout's agent rows by checkout id, the rows its Overview card lists
 * and the rows the sidebar draws under an opened checkout. `agents` is every
 * agent of the project's device.
 */
export function checkoutAgentRows(workspace: Workspace, agents: AgentRow[]): Map<string, BoardRow[]> {
  const { checkoutRows } = lineage(workspace, agents);
  return new Map(workspace.checkouts.map((checkout) => [checkout.id, checkoutRows(checkout)]));
}

/**
 * The board for one Project. `agents` is every agent of the Project's
 * device, since a descendant may work in another project's checkout; only
 * the Project's own panes decide which card an agent belongs to.
 */
export function buildBoard(workspace: Workspace, agents: AgentRow[], now: number): Board {
  const { owners, byPane, treeRows, checkoutRows } = lineage(workspace, agents);

  const git = workspace.is_git === true;
  const tasks: BoardCard[] = [];
  const adHoc: BoardCard[] = [];
  for (const checkout of workspace.checkouts) {
    const rows = checkoutRows(checkout);
    const onBoard = checkout.is_worktree && git;
    const value = card(`task:${checkout.id}`, checkout, rows, checkout.issue ?? null, onBoard ? stageOf(checkout) : null, false, null, now);
    if (onBoard) tasks.push(value);
    else if (rows.length > 0) adHoc.push(value);
  }
  const linked = new Set(workspace.checkouts.flatMap((checkout) => (checkout.issue ? [issueId(checkout.issue)] : [])));
  const issues = workspace.home_issues;
  const github = workspace.checkouts[0]?.github ?? null;
  for (const issue of issues?.issues ?? []) {
    const link = { issue, source: "GitHub" };
    if (issue.state !== "OPEN" || linked.has(issueId(link))) continue;
    tasks.push(card(`issue:${issueId(link)}`, null, [], link, "ready", true, github, now));
  }

  const roots = agents.filter((agent) => {
    const parent = agent.lineage_parent_pane_id;
    if (parent && byPane.has(parent)) return false;
    return treeRows([agent]).some((row) => owners.has(row.agent.pane_id));
  });
  const agentCards = roots.map((root) => {
    const checkout = owners.get(root.pane_id) ?? null;
    return card(`agent:${root.pane_id}`, checkout, treeRows([root]), checkout?.issue ?? null, checkout?.is_worktree ? stageOf(checkout) : null, false, null, now);
  });

  const state: BoardState = projectAgents(workspace, agents).length === 0 ? "empty" : git ? "board" : "adhoc";
  return {
    state,
    tasks: prioritized(tasks),
    adHoc: prioritized(adHoc),
    agents: prioritized(agentCards),
    overflow: issues?.overflow ?? false,
    stats: projectStats(workspace),
  };
}

function issueId(link: IssueLink): string {
  return `${link.issue.reference.repository}#${link.issue.reference.number}`;
}

/** The cards of one Tasks column. */
export function stageCards(board: Board, stage: Stage): BoardCard[] {
  return board.tasks.filter((value) => value.stage === stage);
}

/** The cards of one Agents column, by the root's group. */
export function agentColumnCards(board: Board, column: AgentColumn): BoardCard[] {
  const groups = AGENT_COLUMNS.find((row) => row.column === column)?.groups ?? [];
  return board.agents.filter((value) => groups.includes(value.rows[0]?.agent.group ?? ""));
}
