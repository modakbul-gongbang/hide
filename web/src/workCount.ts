// The number beside the Work tab's name (PRD work-tab-pr-lens B4): the Needs
// You rows of Sessions and of PRs added, an agent that asks being counted
// once when it stands in both. One value so the shell can draw it wherever it
// puts the tool. Folded lines and the sidebar's own count stay out of it.

import { useMemo } from "react";
import { prListModel, type PrScopeProject } from "./prWork";
import { sessionsModel } from "./sessionPanel";
import { boardProjects } from "./navigation";
import { frontDeviceId } from "./devices";
import type { AgentRow, SnapshotRest } from "./snapshot";
import { useShellStore } from "./store";

export type WorkCounts = {
  /** Sessions rows that need the operator (the first group of Sessions). */
  sessions: number;
  /** PRs rows that need the operator, the folded lines not counted. */
  prs: number;
  /** The union, each asking agent once. */
  total: number;
};

/**
 * The counts of the front scope. `only` is the checkout the scope chip
 * narrows to; the tab's number passes null and counts the whole scope.
 */
export function workCounts(rest: SnapshotRest | null, agents: AgentRow[], only: string | null, now: number): WorkCounts {
  const sessions = sessionsModel(rest, agents, only);
  const asking = sessions.scope?.sessions.groups.find((group) => group.group === "needs_you")?.members ?? [];
  const askingPanes = new Set(asking.map((member) => sessions.scope?.members[member]?.pane_id).filter((pane): pane is string => pane !== undefined));
  const projects: PrScopeProject[] = boardProjects(rest, agents, frontDeviceId(rest))
    .filter((item) => !sessions.project || item.workspace.id === sessions.project.id);
  const prs = prListModel(projects, only, now);
  return countAsks(asking.length, askingPanes, prs.needsYou.map((entry) => entry.row.agent_pane));
}

/**
 * The counts of Needs You rows. A PR that stands in Needs You because its
 * agent asks is the same ask Sessions already counted, so its pane (`null`
 * for a PR with no agent) is added to the total only when Sessions lacks it.
 */
export function countAsks(sessions: number, askingPanes: ReadonlySet<string>, prPanes: readonly (string | null)[]): WorkCounts {
  const extra = prPanes.filter((pane) => pane === null || !askingPanes.has(pane));
  return { sessions, prs: prPanes.length, total: sessions + extra.length };
}

/** The tab's counts, recomputed when the snapshot or the agents move. */
export function useWorkCounts(only: string | null): WorkCounts {
  const rest = useShellStore((state) => state.rest);
  const agents = useShellStore((state) => state.agents);
  return useMemo(() => workCounts(rest, agents, only, Date.now()), [rest, agents, only]);
}
