// Frozen request work selection from main 9f144877. Fixture data only.
import type { AgentPullRequest, Task, Workspace } from "../src/snapshot";
import type { RequestRow } from "./legacyRequestList";
import type { LensAgent } from "../src/overviewLens";
import type { RowWork } from "../src/agentScope";
/** The row's chip and how many other live pull requests its `+N` counts; the core put the chip first. */
export function pullRequestChip(pulls: readonly AgentPullRequest[]): { chip: AgentPullRequest; more: number } | null {
  const live = pulls.filter((pull) => pull.live);
  const chip = live[0];
  return chip ? { chip, more: live.length - 1 } : null;
}

/**
 * The row's issues (D-46, D-47): those the chip's pull request closes, then
 * the checkout's own, then those the other pull requests close, each once
 * and only when the project's source has it, so the chip draws what the
 * Issues board draws.
 */
export function rowIssues(row: RequestRow, project: Workspace): Task[] {
  const tasks = new Map((project.tasks?.tasks ?? []).map((task) => [task.key, task]));
  const pulls = row.lens.agent.request?.pull_requests ?? [];
  const chip = pullRequestChip(pulls)?.chip;
  const keys: string[] = [];
  const closing = (pull: AgentPullRequest) => pull.closing_issues.map((reference) => `github:${reference.repository}#${reference.number}`);
  if (chip) keys.push(...closing(chip));
  if (row.lens.task) keys.push(row.lens.task.key);
  for (const pull of pulls) if (pull !== chip) keys.push(...closing(pull));
  const seen = new Set<string>();
  const issues: Task[] = [];
  for (const key of keys) {
    if (seen.has(key)) continue;
    seen.add(key);
    const task = key === row.lens.task?.key ? row.lens.task : tasks.get(key);
    if (task) issues.push(task);
  }
  return issues;
}

/** Folded chips keep open issues and issues closed after this request (D-47, D-43).
 * A later edit to an already closed issue does not make it current work.
 * `rowIssues` keeps the complete history for the expanded row (B56).
 */
export function rowIssueChips(row: RequestRow, project: Workspace): Task[] {
  const requested = row.lens.agent.request?.request?.at_unix_ms ?? 0;
  return rowIssues(row, project).filter((issue) => issue.open || (issue.closed_at_unix_ms != null && issue.closed_at_unix_ms > requested));
}


export function legacyWork(lens: LensAgent, project: Workspace = lens.project): RowWork {
  const row = { lens } as RequestRow;
  const pulls = lens.agent.request?.pull_requests ?? [];
  const shown = pullRequestChip(pulls);
  return { pull: shown ? pulls.indexOf(shown.chip) : null, more: shown?.more ?? 0,
    issues: rowIssues(row, project).map(t => t.key), issue_chips: rowIssueChips(row, project).map(t => t.key) };
}
