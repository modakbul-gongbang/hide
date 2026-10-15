// The Overview's 백로그 tab as data (PRD overview-device-tasks D-09, D-10,
// D-22, B25-B30): each project's unstarted open items, newest first, and the
// items the core is still making. A started item is one a checkout works on
// (its `task_key`); it leaves the backlog and is counted as in progress.

import type { BoardProject } from "./projectBoard";
import type { BacklogFailure, BacklogItem, IssueLabel, Task, TaskSource, Workspace } from "./snapshot";
import { isMemoKey } from "./overviewTasks";

/** A title's longest form (B29): the core refuses more. */
export const BACKLOG_TITLE_LIMIT = 256;

/** Rows a section shows before `나머지 이슈 N ›` (B27). */
export const BACKLOG_SHOWN = 8;

export type BacklogRow = {
  key: string;
  /** `#N` or `L-N`, when the source names one. */
  token: string | null;
  title: string;
  url: string | null;
  labels: readonly IssueLabel[];
  /** When it was made, else last changed; the order and the age. */
  at: number | null;
  local: boolean;
};

export type ProjectBacklog = {
  source: TaskSource | null;
  /** Open items no checkout works on, newest first. */
  rows: BacklogRow[];
  /** Open items a checkout works on (`진행 중 N`). */
  started: number;
};

function rowOf(task: Task): BacklogRow {
  return {
    key: task.key,
    token: task.id,
    title: task.title,
    url: task.url,
    labels: task.labels ?? [],
    at: task.created_at_unix_ms ?? task.updated_at_unix_ms ?? null,
    local: isMemoKey(task.key),
  };
}

/**
 * Whether a failed undo offers 다시 시도: only a refusal is final, since it
 * sent nothing anywhere and would be refused again; Hide's record that could
 * not be read, and every GitHub failure, may clear.
 */
export function undoRetryable(failure: BacklogFailure | null): boolean {
  return failure !== null && failure !== "refused";
}

/** Whether a checkout of the project works on the item: the item is started (B28). */
export function startedKeys(workspace: Pick<Workspace, "checkouts">): Set<string> {
  return new Set(workspace.checkouts.flatMap((checkout) => (checkout.task_key ? [checkout.task_key] : [])));
}

export function backlogOf(workspace: Pick<Workspace, "checkouts" | "tasks">): ProjectBacklog {
  const started = startedKeys(workspace);
  const open = (workspace.tasks?.tasks ?? []).filter((task) => task.open);
  return {
    source: workspace.tasks?.source ?? null,
    rows: open.filter((task) => !started.has(task.key)).map(rowOf).sort((a, b) => (b.at ?? 0) - (a.at ?? 0)),
    started: open.filter((task) => started.has(task.key)).length,
  };
}

/** The 백로그 tab's count: every unstarted open item of the device (B3). */
export function backlogTally(projects: readonly BoardProject[]): { open: number } {
  return { open: projects.reduce((sum, { workspace }) => sum + backlogOf(workspace).rows.length, 0) };
}

/**
 * The rows the core is still making for a project, or failed to, newest
 * first: a made item stays until its source lists it, so the row never blinks
 * out between the create and the next read (B29).
 */
export function pendingItems(items: readonly BacklogItem[] | undefined, workspace: Pick<Workspace, "id" | "tasks">): BacklogItem[] {
  const listed = new Set((workspace.tasks?.tasks ?? []).map((task) => task.key));
  return (items ?? [])
    .filter((item) => item.workspace_id === workspace.id && (item.phase !== "created" || (item.task_key !== null && !listed.has(item.task_key))))
    .sort((a, b) => b.at_unix_ms - a.at_unix_ms);
}

/** The search box and the label chips (B27): words in the token or title, and any one picked label. */
export function filterRows(rows: readonly BacklogRow[], query: string, labels: readonly string[]): BacklogRow[] {
  const needle = query.trim().toLocaleLowerCase();
  return rows.filter((row) => (needle === "" || `${row.token ?? ""}\n${row.title}`.toLocaleLowerCase().includes(needle)) && (labels.length === 0 || row.labels.some((label) => labels.includes(label.name))));
}

/** Every label the device's backlog rows carry, once, by name. */
export function backlogLabels(projects: readonly BoardProject[]): IssueLabel[] {
  const labels = new Map<string, IssueLabel>();
  for (const { workspace } of projects) for (const row of backlogOf(workspace).rows) for (const label of row.labels) if (!labels.has(label.name)) labels.set(label.name, label);
  return [...labels.values()].sort((a, b) => a.name.localeCompare(b.name));
}

/** What the input keeps of typed or pasted text (B29, B42): one line, at most the limit in characters. */
export function backlogTitle(text: string): string {
  return Array.from(text.replace(/\s*[\r\n]+\s*/g, " ")).slice(0, BACKLOG_TITLE_LIMIT).join("");
}

/** The remaining characters the input shows near the limit (B29). */
export function titleLeft(title: string): number {
  return BACKLOG_TITLE_LIMIT - Array.from(title).length;
}

/** One intent key per add, so a retry of the same add converges on one item (D-34). */
export function backlogIntent(): string {
  const bytes = new Uint8Array(18);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (byte) => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"[byte % 64]).join("");
}

/** Where an item goes, for the input's hint and the quick add's line (B26, B39). */
export type Destination = "public" | "private" | "unknown" | "local";

export function destinationOf(source: TaskSource | null): Destination {
  if (!source || source.kind !== "github") return "local";
  return source.public === true ? "public" : source.public === false ? "private" : "unknown";
}
