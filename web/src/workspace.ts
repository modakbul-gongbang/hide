// The Workspace screen's rules (PRD S6 B5-B11, B17; issue 170): which strip
// entries are Agent tabs, what an Agent tab is called and marked with, and how
// the side panel is laid out over the agents. The core owns every value here
// (`rest.workspace_view`, the strip); these functions only read them, so they
// are testable without a page. The View areas' own rules are `viewLayout.ts`.

import type { AgentRow, Checkout, SnapshotRest, StripTab, Tab, ViewLayoutSnapshot } from "./snapshot";

/** How the side panel shows (issue 170): closed, at its width, or over the whole body. */
export type PanelState = "closed" | "open" | "expanded";

/** The front Workspace's presentation as the core published it. */
export type WorkspaceView = {
  device_id: string;
  path: string;
  panel: PanelState;
  /** Docked: the agents end at the panel's left edge rather than running under it. */
  pinned: boolean;
  explorer: boolean;
  changes: boolean;
  /** The open panel's width, as a share of the Workspace body's. */
  views_over_share: number;
  /** The Workspace the operator last chose, now or before a restart, so the page opens on it (D-11). */
  resumed?: boolean;
  /** The View areas (S7); absent only from a core that predates them. */
  layout?: ViewLayoutSnapshot;
};

/** The three panel states, in the order the toolbar menu and the palette offer them: the menu's name for each, and the palette's command to reach it. */
export const PANEL_STATES: readonly { panel: PanelState; label: string; command: string }[] = [
  { panel: "closed", label: "Side panel closed", command: "Close side panel" },
  { panel: "open", label: "Side panel open", command: "Open side panel" },
  { panel: "expanded", label: "Side panel expanded", command: "Expand side panel" },
];

/** The bounds the core keeps the panel's share in (`MIN_VIEWS_OVER_SHARE`, `MAX_VIEWS_OVER_SHARE`). */
export const PANEL_SHARE_MIN = 0.2;
export const PANEL_SHARE_MAX = 0.8;

/** The pixel sizes the panel is laid out with, read from their tokens. */
export type PanelSizes = {
  /** The narrowest the agents left of the panel, or one View area, may be. */
  areaMin: number;
  /** The tool column's width. */
  toolColumn: number;
  /** The narrowest the tool column may be beside a View area before it folds into an overlay. */
  toolMin: number;
};

/**
 * The side panel as the Workspace body draws it (issue 170), from the core's
 * state and the body's width alone; nothing here is sent or stored.
 *
 * - `shown`: what is drawn. A window too narrow for the agents' minimum beside
 *   the panel's draws an open panel over the whole body, like Expanded, and a
 *   pinned one floats there; widening brings back what the core stores.
 * - `content`: the View areas while a view is open or opening, else the tool
 *   column alone, so the panel is only as wide as it, else the empty state.
 * - `width`: the panel's width, and `agentsRight` how far the agents end from
 *   the body's right edge: the open panel's width while it is pinned, even
 *   under an expanded panel, so expanding moves no terminal; else 0, since
 *   the Agent area keeps the body's width under a panel that floats.
 * - `toolsOverlay`: the tools fold into an overlay inside the panel when the
 *   panel cannot give a View area its minimum beside the tool column.
 */
export type PanelFrame = {
  shown: PanelState;
  content: "views" | "tools" | "empty";
  width: number;
  agentsRight: number;
  narrow: boolean;
  resizable: boolean;
  toolsOverlay: boolean;
};

export function panelFrame(input: {
  view: Pick<WorkspaceView, "panel" | "pinned" | "views_over_share" | "explorer" | "changes">;
  /** A view is open, or a file of this checkout is opening into the View areas. */
  views: boolean;
  body: number;
  sizes: PanelSizes;
}): PanelFrame {
  const { view, views, body, sizes } = input;
  const tools = view.explorer || view.changes;
  const content = views ? "views" : tools ? "tools" : "empty";
  if (view.panel === "closed") return { shown: "closed", content, width: 0, agentsRight: 0, narrow: false, resizable: false, toolsOverlay: false };
  const need = panelMinimum(content, tools, sizes);
  const narrow = body > 0 && body < sizes.areaMin + need;
  const open = narrow ? body : content === "tools" ? sizes.toolColumn : panelWidth(view.views_over_share, body, need, sizes.areaMin);
  // With no view to show, the panel stays the tool column's width.
  const expanded = narrow || (view.panel === "expanded" && content !== "tools");
  const width = expanded ? body : open;
  return {
    shown: expanded ? "expanded" : "open",
    content,
    width,
    agentsRight: view.pinned && !narrow ? open : 0,
    narrow,
    resizable: !expanded && content !== "tools",
    toolsOverlay: content === "views" && tools && width < sizes.areaMin + sizes.toolMin,
  };
}

/** The narrowest an open panel may be for what it holds. */
function panelMinimum(content: PanelFrame["content"], tools: boolean, sizes: PanelSizes): number {
  if (content === "tools") return sizes.toolColumn;
  return sizes.areaMin + (content === "views" && tools ? sizes.toolColumn : 0);
}

/**
 * The open panel's width in pixels for a share of a body `total` wide: the
 * share within the core's bounds, and neither the panel narrower than `need`
 * nor the agents left of it narrower than `areaMin`.
 */
export function panelWidth(share: number, total: number, need: number, areaMin: number): number {
  const bounded = Math.min(Math.max(share, PANEL_SHARE_MIN), PANEL_SHARE_MAX);
  return Math.round(Math.min(Math.max(bounded * total, need), total - areaMin));
}

/**
 * The share a panel whose left edge is dragged to `x` pixels from the body's
 * left edge lands at: the width `panelWidth` draws for it, so the core's
 * clamp keeps it where it was released.
 */
export function panelShareAt(x: number, total: number, need: number, areaMin: number): number {
  if (total <= 0) return PANEL_SHARE_MIN;
  return panelWidth((total - x) / total, total, need, areaMin) / total;
}

/** The minimum an open panel holding the View areas needs, for a drag to land within. */
export function panelNeed(tools: boolean, sizes: PanelSizes): number {
  return panelMinimum("views", tools, sizes);
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
