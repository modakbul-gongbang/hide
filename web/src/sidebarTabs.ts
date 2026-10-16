import type { BoardRow } from "./projectBoard";
import type { AgentRow, Checkout, Tab } from "./snapshot";

/** One tab's segment of a checkout's agent list; `glyph` is set when its tab holds two or more recognized agents or the operator named it. */
export type SidebarTabGroup = { tab: Tab | null; rows: BoardRow[]; count: number; glyph: boolean };

/** A tab on the strip the operator named through Rename is kept when its last agent leaves, so it stays a segment of its checkout with no rows; a spawn's name does not keep it. */
function namedOnStrip(tab: Tab): boolean {
  return tab.id !== null && tab.sidebar !== undefined && tab.sidebar.named_by_operator && !tab.delegated;
}

/** Keep complete lineage subtrees with their root, ordered by the actual tabs; a named tab with no agent keeps its segment, empty. */
export function sidebarTabGroups(checkout: Checkout, device: string, rows: readonly BoardRow[], agents: readonly AgentRow[]): SidebarTabGroup[] {
  const agentPanes = new Set(agents.filter((agent) => agent.device_id === undefined || agent.device_id === device).map((agent) => agent.pane_id));
  const paneTabs = new Map(checkout.tabs.flatMap((tab) => tab.panes.map((pane) => [pane.id, tab] as const)));
  const byTab = new Map<Tab | null, BoardRow[]>();
  let owner: Tab | null = null;
  for (const row of rows) {
    if (row.depth === 0) owner = paneTabs.get(row.agent.pane_id) ?? null;
    const group = byTab.get(owner) ?? [];
    group.push(row);
    byTab.set(owner, group);
  }
  return [...checkout.tabs, null].flatMap((tab) => {
    const members = byTab.get(tab) ?? [];
    if (!members.length && !(tab !== null && namedOnStrip(tab))) return [];
    const count = tab ? new Set(tab.panes.filter((pane) => agentPanes.has(pane.id)).map((pane) => pane.id)).size : 0;
    const glyph = tab !== null && tab.id !== null && tab.sidebar !== undefined && (count >= 2 || tab.sidebar.manual_name !== null);
    return [{ tab, rows: members, count, glyph }];
  });
}

/** Up/Down/Home/End traverse only the visible main row controls. */
export function moveSidebarKey(event: import("react").KeyboardEvent<HTMLElement>): void {
  if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
  if (!["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
  const target = event.target;
  if (!(target instanceof HTMLElement) || !target.matches("[data-agent-open], [data-sidebar-tab]")) return;
  const rows = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("[data-agent-open]:not(:disabled), [data-sidebar-tab]:not(:disabled)")];
  const index = rows.indexOf(target as HTMLButtonElement);
  if (index < 0) return;
  const next = event.key === "Home" ? 0 : event.key === "End" ? rows.length - 1 : Math.max(0, Math.min(rows.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)));
  event.preventDefault();
  rows[next]?.focus();
}
