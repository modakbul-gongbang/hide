// The Workspace screen's rules (PRD S6 B5-B11, B17; issue 170): which strip
// entries are Agent tabs, what an Agent tab is called and marked with, and how
// the side panel is laid out over the agents. The core owns every value here
// (`rest.workspace_view`, the strip); these functions only read them, so they
// are testable without a page. The View areas' own rules are `viewLayout.ts`.

import type { AgentRow, Checkout, SnapshotRest, StripTab, Tab, ViewLayoutSnapshot } from "./snapshot";

/** How the side panel shows (issue 170): closed, at its width, or over the whole body. */
export type PanelState = "closed" | "open" | "expanded";

/** The side panel's tools (issue 170): the Explorer, or History (`changes`). The column holds one at a time. */
export type Tool = "explorer" | "changes";

/** The front Workspace's presentation as the core published it. */
export type WorkspaceView = {
  device_id: string;
  path: string;
  panel: PanelState;
  /** Docked: the agents end at the panel's left edge rather than running under it. */
  pinned: boolean;
  /** The one tool the tool column holds, kept while the column is hidden. */
  tool: Tool;
  /** Whether the tool column shows. */
  tools: boolean;
  /** The open panel's width, as a share of the Workspace body's. */
  views_over_share: number;
  /** A panel holding only the tools: its width as a share of the body's, or null for the tool column's own width until it is resized. */
  tools_share: number | null;
  /** A page reported that this panel takes the whole body when it shows (a narrow window). */
  covered: boolean;
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
  /** What the panel's frame takes from its width: the gap on its left and the card's two side hairlines. */
  chrome: number;
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
 * - `resize`: which stored width a drag on the panel's edge sets: the View
 *   areas' and the empty panel's share, or the tools-only panel's own, which
 *   is the tool column's width until it is first resized; null while the
 *   panel covers the body. `need` is the narrowest the panel may be.
 * - `toolsOverlay`: in a window too narrow for both, the tools fold into an
 *   overlay inside the panel; anywhere else the panel's minimum already holds
 *   a View area beside the tool column.
 */
export type PanelFrame = {
  shown: PanelState;
  content: "views" | "tools" | "empty";
  width: number;
  agentsRight: number;
  narrow: boolean;
  resize: "views_over_share" | "tools_share" | null;
  need: number;
  toolsOverlay: boolean;
};

export function panelFrame(input: {
  view: Pick<WorkspaceView, "panel" | "pinned" | "views_over_share" | "tools_share" | "tools">;
  /** A view is open, or a file of this checkout is opening into the View areas. */
  views: boolean;
  body: number;
  sizes: PanelSizes;
}): PanelFrame {
  const { view, views, body, sizes } = input;
  const tools = view.tools;
  const content = views ? "views" : tools ? "tools" : "empty";
  const need = panelMinimum(content, tools, sizes);
  if (view.panel === "closed") return { shown: "closed", content, width: 0, agentsRight: 0, narrow: false, resize: null, need, toolsOverlay: false };
  // Before the body is measured nothing is placed, so no terminal fits to a guess.
  if (body <= 0) return { shown: view.panel === "expanded" && content === "views" ? "expanded" : "open", content, width: 0, agentsRight: 0, narrow: false, resize: null, need, toolsOverlay: false };
  const narrow = body < sizes.areaMin + need;
  const toolsOnly = content === "tools";
  const share = toolsOnly ? view.tools_share : view.views_over_share;
  const open = narrow ? body : share === null ? need : panelWidth(share, body, need, sizes.areaMin);
  // Only views expand: with none, the panel stays the tool column's width or
  // its own at the empty state, and the agents stay in reach.
  const expanded = narrow || (view.panel === "expanded" && content === "views");
  const width = expanded ? body : open;
  return {
    shown: expanded ? "expanded" : "open",
    content,
    width,
    agentsRight: view.pinned && !narrow ? open : 0,
    narrow,
    resize: expanded ? null : toolsOnly ? "tools_share" : "views_over_share",
    need,
    toolsOverlay: content === "views" && tools && narrow,
  };
}

/** The narrowest an open panel may be for what it holds. */
function panelMinimum(content: PanelFrame["content"], tools: boolean, sizes: PanelSizes): number {
  if (content === "tools") return sizes.chrome + sizes.toolColumn;
  return sizes.chrome + sizes.areaMin + (content === "views" && tools ? sizes.toolColumn : 0);
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

/**
 * Whether a page reports `panel_covers` now (issue 170): only when what it
 * draws differs from what the core holds and from its own last report, so a
 * crossing is sent once, a report the page forgot (another Workspace drawn
 * since, or the connection lost) is sent again, and two pages that disagree
 * each send once rather than undoing each other forever.
 */
export function panelCoversToSend(covers: boolean, core: boolean, lastSent: boolean | null): boolean {
  return covers !== core && covers !== lastSent;
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
