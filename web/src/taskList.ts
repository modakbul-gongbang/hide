// The sidebar's 태스크 list as data (PRD sidebar-task-lens B4-B9): the rail's
// device's tasks in the core's order (`agent_scope.tasks` of the device),
// cut into the groups the list draws. The core decides each task's state and
// order; this only places a task under its state or its project and folds
// the resting and cleanup ones.

import type { ProjectTasksScope, TaskCardScope } from "./agentScope";
import type { AgentRow, TaskView, Workspace } from "./snapshot";

/**
 * The open groups of the 상태별 view and the bands of a 프로젝트별 group, in the
 * order they are drawn (B4, B6): the groups that want the operator's look
 * first, so 끝남 stands above 일하는 중, whose tasks need nothing from them yet,
 * and 기다림 last, whose tasks wait on a descendant or an answer.
 */
export type TaskGroupState = "needs_you" | "blocked" | "done" | "working" | "waiting";
export const TASK_GROUPS: readonly TaskGroupState[] = ["needs_you", "blocked", "done", "working", "waiting"];

export type TaskEntry = { card: TaskCardScope; agent: AgentRow; project: Workspace | null };

export type TaskGroup = {
  /** `state:<state>` or `project:<id>`, also the folds' key. */
  key: string;
  state: TaskGroupState | null;
  project: Workspace | null;
  /** The open rows, band by band, each band in the core's order (B9). */
  entries: TaskEntry[];
  /** The open rows cut by state, in `TASK_GROUPS` order; a 상태별 group has one band, a 프로젝트별 group draws each under its state (B6). */
  bands: { state: TaskGroupState; entries: TaskEntry[] }[];
  /** Per state, the open rows a project's head counts (B6). */
  counts: Partial<Record<TaskGroupState, number>>;
  resting: TaskEntry[];
  cleanup: TaskEntry[];
};

export type TaskList = {
  /** Open tasks, the folded ones not counted. */
  total: number;
  groups: TaskGroup[];
  /** The 상태별 view's folds, drawn once under every group. */
  resting: TaskEntry[];
  cleanup: TaskEntry[];
};


/**
 * The list for one view. A card whose root row the snapshot no longer holds
 * is left out until it does, rather than drawn without a title.
 */
export function taskList(tasks: ProjectTasksScope | null | undefined, rows: ReadonlyMap<string, AgentRow>, projects: readonly Workspace[], view: TaskView): TaskList {
  const byCheckout = new Map(projects.flatMap((project) => project.checkouts.map((checkout) => [checkout.id, project] as const)));
  const entries: TaskEntry[] = [];
  for (const card of tasks?.cards ?? []) {
    const agent = rows.get(card.root);
    if (!agent) continue;
    entries.push({ card, agent, project: card.checkouts.map((id) => byCheckout.get(id)).find((row) => row !== undefined) ?? null });
  }
  const open = entries.filter(({ card }) => card.state !== "resting" && card.state !== "cleanup");
  const resting = entries.filter(({ card }) => card.state === "resting");
  const cleanup = entries.filter(({ card }) => card.state === "cleanup");
  if (view === "status") {
    const groups = TASK_GROUPS.map((state) => {
      const entries = open.filter(({ card }) => card.state === state);
      return { key: `state:${state}`, state, project: null, entries, bands: [{ state, entries }], counts: {}, resting: [], cleanup: [] };
    }).filter((group) => group.entries.length > 0);
    return { total: open.length, groups, resting, cleanup };
  }
  // One group per project, the projects that ask first, then in the order their first task stands (B6).
  const order: (Workspace | null)[] = [];
  for (const { project } of entries) if (!order.includes(project)) order.push(project);
  const asks = (project: Workspace | null) => open.some((entry) => entry.project === project && entry.card.state === "needs_you");
  const sorted = [...order.filter(asks), ...order.filter((project) => !asks(project))];
  const groups = sorted.map((project) => {
    const mine = (list: TaskEntry[]) => list.filter((entry) => entry.project === project);
    const shown = mine(open);
    const bands = TASK_GROUPS.map((state) => ({ state, entries: shown.filter(({ card }) => card.state === state) })).filter((band) => band.entries.length > 0);
    const counts: Partial<Record<TaskGroupState, number>> = Object.fromEntries(bands.map((band) => [band.state, band.entries.length]));
    return { key: `project:${project?.id ?? ""}`, state: null, project, entries: bands.flatMap((band) => band.entries), bands, counts, resting: mine(resting), cleanup: mine(cleanup) };
  });
  return { total: open.length, groups, resting: [], cleanup: [] };
}
