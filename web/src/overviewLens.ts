// The agents a scope holds and where each stands, how old a value is, and
// which worktrees are only there to be cleaned, as pure functions over the
// snapshot.

import type { TFunction } from "i18next";
import { formatRelativeTime } from "./i18n/format";
import type { InterfaceLanguage } from "./i18n/locale";
import { stageOf, type BoardProject } from "./projectBoard";
import type { AgentRow, Checkout, Task, Workspace } from "./snapshot";

// --- agents ------------------------------------------------------------------

/**
 * Where an agent stands for the operator (D-37, D-38): its turn (asking, or
 * finished and not looked at), working, waiting on its children, or resting.
 * The graph's status chips and the Agents tile read this one order.
 */
export type AgentBucket = "turn" | "working" | "delegating" | "resting";

export function bucketOf(agent: AgentRow): AgentBucket {
  return agent.state.bucket;
}

/** One agent the Overview draws, with where it works. */
export type LensAgent = {
  agent: AgentRow;
  bucket: AgentBucket;
  project: Workspace;
  checkout: Checkout;
  /** The SSH device it runs on, or null for this machine. */
  device: string | null;
  /** The issue its checkout works on. */
  task: Task | null;
};

/** Every agent whose pane one of the scope's checkouts holds, in the core's order. */
export function scopeAgents(projects: readonly BoardProject[]): LensAgent[] {
  const result: LensAgent[] = [];
  for (const { workspace, agents, device } of projects) {
    const byPane = new Map([...agents].reverse().map((agent) => [agent.pane_id, agent]));
    const checkouts = new Map(workspace.checkouts.map((checkout) => [checkout.id, checkout]));
    const tasks = new Map((workspace.tasks?.tasks ?? []).map((task) => [task.key, task]));
    for (const member of workspace.agent_scope.members) {
      const agent = byPane.get(member.pane_id);
      const checkout = checkouts.get(member.checkout_id);
      if (!agent || !checkout) throw new Error("Overview scope references a missing agent or checkout");
      result.push({ agent, bucket: bucketOf(agent), project: workspace, checkout, device, task: checkout.task_key ? (tasks.get(checkout.task_key) ?? null) : null });
    }
  }
  return result;
}

/** The age a failure's popover gives its last value: the language's own relative form, and `overview.justNow` under a minute. */
export function ageWords(language: InterfaceLanguage, ms: number, t: TFunction<"translation">): string {
  const minutes = Math.max(0, Math.floor(ms / 60_000));
  if (minutes < 1) return t("overview.justNow");
  if (minutes < 60) return formatRelativeTime(language, -minutes, "minute");
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return formatRelativeTime(language, -hours, "hour");
  return formatRelativeTime(language, -Math.floor(hours / 24), "day");
}

// --- worktree cleanup --------------------------------------------------------

/** Why a worktree is only there to be removed: its work is merged, or its folder is gone. */
export type Cleanup = "merged" | "missing";

export function cleanupOf(workspace: Workspace, checkout: Checkout): Cleanup | null {
  if (!checkout.is_worktree || checkout.is_primary === true || workspace.is_git !== true) return null;
  if (!checkout.exists || checkout.worktree?.missing === true) return "missing";
  return stageOf(checkout) === "done" ? "merged" : null;
}
