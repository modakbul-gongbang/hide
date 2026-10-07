// Frozen PR board fixture rules from main 9f144877.
import type { AgentRow, Workspace, Checkout, Task, PullRequest } from "../src/snapshot";
import type { BoardRow, BoardProject, PrTone, ReadFailure } from "../src/projectBoard";
import type { MessageKey } from "../src/i18n/catalogs";
import { prChip } from "../src/projectBoard";
import { checkoutAgentRows } from "./legacyAgentTree";
// --- pull requests (PRD overview-lenses-prs) -------------------------------------

/** Whose move a pull request waits on (D-11, D-32): the operator's, an agent fixing it, nobody though it is blocked, or merged. */
export type PrGroup = "turn" | "fixing" | "blocked" | "merged";

export const PR_GROUP_LABEL: Record<PrGroup, MessageKey> = {
  turn: "board.prGroup.turn",
  fixing: "board.prGroup.fixing",
  blocked: "board.prGroup.blocked",
  merged: "board.prGroup.merged",
};

const PR_GROUPS: readonly PrGroup[] = ["turn", "fixing", "blocked", "merged"];

/** An agent at work, a parent waiting on its working children included (status-model's activity axis, PRD Risks). */
function isWorking(agent: AgentRow): boolean {
  return agent.state.working;
}

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
  groups: { group: PrGroup; rows: PrRow[] }[];
  /** Open pull requests, or null until GitHub has answered (B22). */
  open: number | null;
  /** `gh` has not answered yet for this project (B22). */
  reading: boolean;
  /** That the last read failed, with the value's age; the reason is in the log (B22). */
  failure: ReadFailure | null;
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
  const rowsByCheckout = checkoutAgentRows(workspace, agents);
  const local = workspace.tasks?.source?.kind === "local";
  const rows: PrRow[] = [];
  for (const pr of workspace.pull_requests ?? []) {
    const branch = pr.head_branch ?? "";
    // The checkout the core connected it to (a settled pull request only at the
    // exact commit), never a branch of the same name: a branch used again for
    // new work is not the merged pull request's worktree to clean up.
    const checkout = workspace.checkouts.find((row) => row.pull_request?.url === pr.url && (row.is_worktree || row.exists)) ?? null;
    const boardRows = checkout ? (rowsByCheckout.get(checkout.id) ?? []) : [];
    const panes = new Set(checkout?.tabs.flatMap((tab) => tab.panes.map((pane) => pane.id)) ?? []);
    // The agent whose session made it is on its row too, wherever it works
    // (overview-request-view D-45): the request view and this board share the link.
    const made = (agent: AgentRow) => agent.request?.pull_requests.some((pull) => pull.created && pull.url === pr.url) === true;
    const agentsHere = agents.filter((agent) => panes.has(agent.pane_id) || made(agent)).sort((a, b) => PR_ATTENTION(a) - PR_ATTENTION(b));
    // Whose move it is stays the branch's: the maker may be at other work by now.
    const onBranch = agentsHere.filter((agent) => panes.has(agent.pane_id));
    const chip = prChip(pr);
    const merged = pr.badge === "merged";
    const checks = chip.checks;
    const review = merged ? null : pr.review;
    const group: PrGroup = merged
      ? "merged"
      : onBranch.some(isWorking)
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
      needsLook: onBranch.some((agent) => agent.group === "done"),
      checks,
      review,
      at: merged ? (pr.merged_at_unix_ms ?? null) : (pr.updated_at_unix_ms ?? null),
      delegate: group === "blocked",
      linkable: !merged && issue === null && (!local || (worktree && !folderGone)),
      cleanup: merged && worktree ? (folderGone ? "record" : "worktree") : null,
    });
  }
  const byRecent = (a: PrRow, b: PrRow) => (b.at ?? 0) - (a.at ?? 0) || b.number - a.number;
  const groups = PR_GROUPS.map((group) => ({ group, rows: rows.filter((row) => row.group === group).sort(byRecent) })).filter((entry) => entry.rows.length > 0);
  const status = workspace.checkouts.find((checkout) => checkout.github)?.github ?? null;
  // A repository with no GitHub remote has no pull requests, which is an answer, not a failure (design 13).
  const noRemote = status?.failure_category === "no_github_remote";
  const answered = status?.last_success_at_unix_ms != null || noRemote;
  const failed = !noRemote && (status?.stale === true || (status?.unavailable_reason != null && !status.available));
  const age = status?.last_success_at_unix_ms != null ? Math.max(0, Math.floor((now - status.last_success_at_unix_ms) / 60_000)) : null;
  return {
    groups,
    open: answered ? rows.filter((row) => row.group !== "merged").length : null,
    reading: !answered && !failed,
    failure: failed ? { project: null, source: "GitHub", value: age === null ? "none" : { minutes: age } } : null,
  };
}
