// ⌘⇧M's project choice as data (PRD overview-device-tasks D-12, D-33, B38):
// the projects of the device in front that keep a backlog, and which one is
// picked first and why.

import { readsHere } from "./devices";
import type { OverviewPlace } from "./ui";
import type { SnapshotRest, Workspace } from "./snapshot";

export type QuickAddReason = "task" | "filter" | "front" | "first";

export type QuickAddChoice = { projects: Workspace[]; index: number; reason: QuickAddReason };

/**
 * `device` is the one the Overview shows while it is on screen, else the one
 * in front; `overview` is the Overview's place while it is on screen, null
 * otherwise; `taskProject` is the project of the task card its panel shows,
 * which only the snapshot's agents can tell. A project the core dials has no
 * issue source, so it is never offered (D-33).
 */
export function quickAddChoice(rest: SnapshotRest | null, workspaces: readonly Workspace[], device: string, overview: OverviewPlace | null, taskProject: string | null, frontWorkspace: string | null): QuickAddChoice {
  const devices = rest?.navigator?.devices;
  const projects = workspaces.filter((workspace) => workspace.device_id === device && readsHere(workspace, devices) && workspace.tasks?.source != null);
  const panel = overview?.panel;
  const picked: [string | null | undefined, QuickAddReason][] = [
    [panel?.kind === "item" ? panel.workspaceId : panel?.kind === "task" || panel?.kind === "record" ? taskProject : null, "task"],
    [overview?.projectId, "filter"],
    [frontWorkspace, "front"],
  ];
  for (const [id, reason] of picked) {
    const index = id ? projects.findIndex((project) => project.id === id) : -1;
    if (index >= 0) return { projects, index, reason };
  }
  return { projects, index: 0, reason: "first" };
}

/** Tab and Shift+Tab: the next or previous project, round. */
export function cycleIndex(index: number, count: number, backward: boolean): number {
  if (count === 0) return 0;
  return (index + (backward ? count - 1 : 1)) % count;
}
