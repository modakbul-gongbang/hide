// The Workspace screen's rules (PRD S6 B5-B11, B17): which strip entries are
// Agent tabs and which are View tabs, which View tab shows, what a tab is
// called and marked with, and where the Agent/View boundary may sit. The core
// owns every value here (`rest.workspace_view`, the strip, the editor's active
// tab); these functions only read them, so they are testable without a page.

import type { AgentRow, Checkout, EditorSnapshot, EditorTabSnapshot, SnapshotRest, StripTab, Tab, ViewLayoutSnapshot } from "./snapshot";

export type ViewMode = "agents" | "together" | "views";

/** The front Workspace's presentation as the core published it. */
export type WorkspaceView = {
  device_id: string;
  path: string;
  mode: ViewMode;
  explorer: boolean;
  changes: boolean;
  agent_share: number;
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

export function workspaceViewOf(rest: SnapshotRest | null): WorkspaceView | null {
  return (rest?.workspace_view as WorkspaceView | undefined) ?? null;
}

/** Herdr tabs: the Agent area's strip, in the core's order. */
export function agentEntries(checkout: Checkout): StripTab[] {
  return checkout.strip.filter((entry) => entry.kind === "herdr");
}

/** Files and diffs: the View area's strip, in the core's order. */
export function viewEntries(checkout: Checkout): StripTab[] {
  return checkout.strip.filter((entry) => entry.kind === "file" || entry.kind === "diff");
}

/**
 * The View tab the View area shows: the editor's active tab when it is a file
 * or diff of this checkout. The core keeps it on the front Workspace's View
 * tab, so a terminal choice never empties the View area (D-04).
 */
export function activeViewTab(editor: EditorSnapshot | null, checkout: Checkout | null): EditorTabSnapshot | null {
  if (!editor?.active_tab_id || !checkout) return null;
  const tab = editor.tabs.find((row) => row.id === editor.active_tab_id);
  if (!tab || (tab.kind !== "file" && tab.kind !== "diff")) return null;
  return tab.checkout_id === checkout.id ? tab : null;
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

/** What a tab is, read in its tooltip and by assistive technology with its full name (B17). */
export function tabIdentity(entry: StripTab, agent: AgentRow | null, editorTab: EditorTabSnapshot | null): string {
  if (entry.kind === "herdr") {
    const kind = agent ? `${agent.agent_kind} agent` : "Terminal";
    const pane = agent ? ` · ${agent.identity_label} · ${agent.status_label}` : "";
    return `${kind} tab ${entry.label}${pane}`;
  }
  if (!editorTab) return entry.label;
  const kind = entry.kind === "diff" ? `${editorTab.diff_committed ? "Branch" : "Working"} diff` : "File";
  const state = editorTab.unavailable_reason ? " · Unavailable" : entry.preview ? " · Preview" : "";
  return `${kind}: ${editorTab.path}${state}`;
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
