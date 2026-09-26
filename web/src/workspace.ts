// The Workspace screen's rules (PRD S6 B5-B11, B17): which strip entries are
// Agent tabs, what an Agent tab is called and marked with, and where the
// Agent/View boundary may sit. The core owns every value here
// (`rest.workspace_view`, the strip); these functions only read them, so they
// are testable without a page. The View areas' own rules are `viewLayout.ts`.

import type { AgentRow, Checkout, SnapshotRest, StripTab, Tab, ViewLayoutSnapshot } from "./snapshot";

export type ViewMode = "agents" | "together" | "views";

/** The front Workspace's presentation as the core published it. */
export type WorkspaceView = {
  device_id: string;
  path: string;
  mode: ViewMode;
  explorer: boolean;
  changes: boolean;
  agent_share: number;
  /** Agents only with the View areas drawn over the Agent area (issue 170); absent from a core that predates it. */
  views_over_agents?: boolean;
  /** The Workspace the operator last chose, now or before a restart, so the page opens on it (D-11). */
  resumed?: boolean;
  /** The View areas (S7); absent only from a core that predates them. */
  layout?: ViewLayoutSnapshot;
};

/** The three layouts, in the order the toolbar and the menu offer them (D-03). */
export const LAYOUTS: readonly { mode: ViewMode; label: string; description: string }[] = [
  { mode: "agents", label: "Agents only", description: "Show only the agent tabs and their panes" },
  { mode: "together", label: "Agents and Views", description: "Show agents and documents side by side" },
  { mode: "views", label: "Views only", description: "Show only the open files and diffs" },
];

export function layoutLabel(mode: ViewMode): string {
  return LAYOUTS.find((layout) => layout.mode === mode)?.label ?? mode;
}

/**
 * The layout the Workspace body draws. It is the stored mode, except that View
 * areas with no display and no file of this checkout opening take no space:
 * Together draws the agents alone and Views only draws the agents in their
 * place (B8, B10). The stored mode is untouched, so the next display brings
 * the View areas back in it (B9).
 */
export function drawnMode(view: WorkspaceView, opening: boolean): ViewMode {
  if (view.mode === "agents" || opening || !view.layout) return view.mode;
  return view.layout.display_count > 0 ? view.mode : "agents";
}

/**
 * Whether the View areas are drawn over the Agent area (issue 170): Agents
 * only with the core's `views_over_agents` up, while they hold a display or a
 * file of this checkout is opening into them. The Agent area keeps its size
 * underneath, so no terminal resizes when they come or go.
 */
export function viewsOverAgents(view: WorkspaceView, opening: boolean): boolean {
  if (view.mode !== "agents" || !view.views_over_agents) return false;
  return opening || !view.layout || view.layout.display_count > 0;
}

/**
 * Whether Agents only has View areas to draw over the agents, a display or a
 * file of this checkout opening into them: the toolbar and the palette offer
 * the toggle only then, and always while `viewsOverAgents` draws them.
 */
export function canShowViewsOverAgents(view: WorkspaceView, opening: boolean): boolean {
  return view.mode === "agents" && (opening || (view.layout?.display_count ?? 0) > 0);
}

export function workspaceViewOf(rest: SnapshotRest | null): WorkspaceView | null {
  return (rest?.workspace_view as WorkspaceView | undefined) ?? null;
}

/** Herdr tabs: the Agent area's strip, in the core's order. */
export function agentEntries(checkout: Checkout): StripTab[] {
  return checkout.strip.filter((entry) => entry.kind === "herdr");
}

/** The agent kinds whose own mark Hide ships; any other agent is drawn with the neutral mark (D-09). */
export const KNOWN_PROVIDERS = ["claude", "codex"] as const;
export type Provider = (typeof KNOWN_PROVIDERS)[number];

export function knownProvider(kind: string | null | undefined): Provider | null {
  return (KNOWN_PROVIDERS as readonly string[]).includes(kind ?? "") ? (kind as Provider) : null;
}

/**
 * The agent a Herdr tab stands for: the agent of the pane Herdr focused in it
 * when that pane has one, else the first pane that does. A tab of plain
 * shells stands for none and wears the neutral terminal mark.
 */
export function tabAgent(tab: Tab | null | undefined, agents: AgentRow[], focusedPaneId: string | null): AgentRow | null {
  if (!tab) return null;
  const ids = tab.panes.map((pane) => pane.id);
  const byPane = new Map(agents.map((agent) => [agent.pane_id, agent]));
  if (focusedPaneId && ids.includes(focusedPaneId)) {
    const focused = byPane.get(focusedPaneId);
    if (focused) return focused;
  }
  for (const id of ids) {
    const agent = byPane.get(id);
    if (agent) return agent;
  }
  return null;
}

/** What an Agent tab is, read in its tooltip and by assistive technology with its full name (B17). */
export function tabIdentity(entry: StripTab, agent: AgentRow | null): string {
  const kind = agent ? `${agent.agent_kind} agent` : "Terminal";
  const pane = agent ? ` · ${agent.identity_label} · ${agent.status_label}` : "";
  return `${kind} tab ${entry.label}${pane}`;
}

/**
 * The Agent area's width in pixels for a share of the body, kept so neither
 * area is narrower than `minimum` (B6). A body too narrow for two minimums
 * splits evenly rather than hiding either area: the layout stays the one the
 * operator chose.
 */
export function agentWidth(share: number, total: number, minimum: number): number {
  if (total <= minimum * 2) return Math.round(total / 2);
  return Math.round(Math.min(Math.max(share * total, minimum), total - minimum));
}

/** The share a boundary dragged to `x` pixels from the body's left edge stands for. */
export function shareAt(x: number, total: number, minimum: number): number {
  if (total <= 0) return 0.5;
  return agentWidth(x / total, total, minimum) / total;
}
