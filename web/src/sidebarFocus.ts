import type { AgentScope } from "./agentScope";
import type { AgentRow, Workspace } from "./snapshot";

/**
 * Every pane the sidebar draws a row for when nothing is folded: each
 * checkout's roots, the Home's roots, and the raised Needs You and Done rows.
 */
export function sidebarPanes(workspaces: readonly Workspace[], scope: AgentScope | null): Set<string> {
  const panes = new Set<string>();
  for (const workspace of workspaces) {
    const trees = workspace.is_home ? [workspace.agent_scope.sidebar_tree] : workspace.checkouts.map((checkout) => checkout.agent_scope.sidebar_tree);
    for (const tree of trees) for (const row of tree.rows) panes.add(row.pane_id);
  }
  for (const raised of scope?.raised ?? []) for (const ref of [...raised.shown, ...raised.more]) panes.add(ref.pane_id);
  return panes;
}

/**
 * The pane whose row stands for the focused pane: the focused pane when the
 * sidebar draws it, otherwise its nearest ancestor that it draws, since a
 * delegated child is reached through its parent's badge (docs/UI_BEHAVIOR.md,
 * Sidebar hierarchy). Null when no pane on that line is drawn.
 */
export function sidebarFocusPane(focused: string | null, agents: readonly AgentRow[], drawn: ReadonlySet<string>): string | null {
  const parents = new Map(agents.map((agent) => [agent.pane_id, agent.lineage_parent_pane_id ?? null]));
  const seen = new Set<string>();
  for (let pane = focused; pane !== null && !seen.has(pane); pane = parents.get(pane) ?? null) {
    if (drawn.has(pane)) return pane;
    seen.add(pane);
  }
  return null;
}

/**
 * Scrolls the list by as little as brings the focus's row into view, and not
 * at all while one of its rows is already in view. The row is the deepest one
 * marked current: an agent row, else the focused checkout's row, else the
 * project or fold row that hides it; an agent drawn both raised and in its
 * tree is brought in at its tree row.
 */
export function revealSidebarFocus(list: HTMLElement): void {
  const marked = [...list.querySelectorAll<HTMLElement>('[aria-current="true"]')];
  if (marked.length === 0) return;
  const view = list.getBoundingClientRect();
  const deepest = ["[data-agent-open]", "[data-checkout]"].map((kind) => marked.filter((row) => row.matches(kind))).find((rows) => rows.length > 0) ?? marked;
  if (deepest.some((row) => inside(row.getBoundingClientRect(), view))) return;
  const target = deepest.find((row) => row.closest("[data-raised-group]") === null) ?? deepest[0]!;
  target.scrollIntoView({ block: "nearest" });
}

function inside(row: DOMRect, view: DOMRect): boolean {
  return row.top >= view.top && row.bottom <= view.bottom;
}
