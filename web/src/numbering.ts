import { deviceScope, scopeOccurrences } from "./agentScope";
// What ⌘n and ⌥n select, and the number each tab and agent row carries
// while a hold reveals it (PRD electron-digit-shortcuts-hints D-02). The
// number is the screen order at that moment, first to ninth: the strip's
// tabs left to right, the Agents list's drawn rows top to bottom. A tenth
// item has no number, and a number with nothing at it selects nothing.

import { areasOf } from "./areaLayout";
import type { AgentLayout } from "./agentLayout";
import type { TreeRow } from "./agentRow";
import { frontDeviceId } from "./devices";
import { deviceListedAgents } from "./navigation";
import { projectRows, activeCheckouts, inactiveCheckouts, folderCheckout } from "./projects";
import { checkoutAgentRows } from "./projectBoard";
import { contextWorkspaces, contextAgents, contextHome } from "./remote";
import { DIGITS, type Digit } from "./shortcuts";
import type { AgentRow, Checkout, SnapshotRest } from "./snapshot";
import { agentEntries } from "./workspace";

/** Local area tree order, then each bar left to right; devices retain their strip order. */
export function numberedTabs(checkout: Checkout, layout?: AgentLayout | null): Map<Digit, string> {
  const numbered = new Map<Digit, string>();
  const entries = agentEntries(checkout);
  const placed = layout ? areasOf(layout.root).flatMap((area) => area.displays.map((item) => item.id)) : entries.map((entry) => entry.source_id);
  placed.filter((id) => entries.some((entry) => entry.source_id === id))
    .slice(0, DIGITS.length)
    .forEach((id, index) => numbered.set(DIGITS[index]!, id));
  return numbered;
}

/** The pane id at each number of the Agents list's drawn rows, in number order. */
export function numberedAgents(rows: readonly TreeRow[]): Map<Digit, string> {
  const numbered = new Map<Digit, string>();
  const seen = new Set<string>();
  rows.filter((row) => !seen.has(row.agent.pane_id) && !!seen.add(row.agent.pane_id)).slice(0, DIGITS.length).forEach((row, index) => numbered.set(DIGITS[index]!, row.agent.pane_id));
  return numbered;
}

/** The number an item holds, by its id, or null past the ninth. */
export function numberOf(numbered: Map<Digit, string>, id: string): Digit | null {
  for (const [digit, held] of numbered) if (held === id) return digit;
  return null;
}

/** The Agents list's rows in draw order, the list ⌥n numbers whichever list is on screen (B2): the front device's. */
function sidebarAgentOrder(state: { rest: SnapshotRest | null; agents: AgentRow[] }, raisedOpen: readonly string[]): { row: TreeRow; place: string | null }[] {
  const { rest } = state;
  const scope = deviceScope(rest, frontDeviceId(rest));
  const agents = contextAgents(rest, state.agents);
  const workspaces = contextWorkspaces(rest);
  const listed = deviceListedAgents(rest?.status?.remote, rest?.navigator?.devices, state.agents, frontDeviceId(rest));
  const projects = projectRows(workspaces, rest?.navigator?.inactive_projects ?? [], listed, scope, raisedOpen);
  const rows: { row: TreeRow; place: string | null }[] = [];
  const append = (agent: AgentRow, place: string | null = null) => rows.push({ row: { agent, device: null, depth: 0, descendants: (agent.lineage_child_pane_ids ?? []).length }, place });
  for (const row of projects) if (row.kind === "raised") for (const item of [...row.agents, ...(row.expanded ? row.more : [])]) append(item.agent);
  const home = contextHome(rest);
  if (home) for (const agent of scopeOccurrences(home.agent_scope.sidebar_tree.rows, agents)) append(agent, "home");
  const collapsed = rest?.ui_state?.session_collapsed_checkout_ids ?? [];
  const checkout = (workspace: import("./snapshot").Workspace, value: Checkout) => {
    for (const row of checkoutAgentRows(workspace, agents, "sidebar").get(value.id) ?? []) {
      if (!collapsed.includes(value.id) || row.agent.state.needs_you) append(row.agent, value.id);
    }
  };
  for (const row of projects) {
    if (row.kind !== "workspace") continue;
    const workspace = row.workspace;
    const folder = folderCheckout(workspace);
    if (folder) {
      if (!workspace.session_folds?.cleanup.includes(folder.id)) checkout(workspace, folder);
      continue;
    }
    if (workspace.expanded === false) continue;
    for (const value of activeCheckouts(workspace)) checkout(workspace, value);
    if (workspace.session_folds?.empty_open) for (const value of workspace.checkouts.filter((value) => workspace.session_folds!.empty.includes(value.id))) checkout(workspace, value);
    if (workspace.inactive_checkouts.expanded) for (const value of inactiveCheckouts(workspace)) checkout(workspace, value);
  }
  for (const workspace of workspaces) if (workspace.session_folds?.cleanup_open) for (const value of workspace.checkouts.filter((value) => workspace.session_folds!.cleanup.includes(value.id))) checkout(workspace, value);
  return rows;
}

/** Sidebar order is the actual drawn order, including each open fold. */
export function agentListOrder(state: { rest: SnapshotRest | null; agents: AgentRow[] }, raisedOpen: readonly string[] = []): TreeRow[] {
  return sidebarAgentOrder(state, raisedOpen).map(({ row }) => row);
}

/** A physical pane gets a keycap only at its first visible appearance. */
export function sidebarAgentNumbers(state: { rest: SnapshotRest | null; agents: AgentRow[] }, raisedOpen: readonly string[]) {
  const entries = sidebarAgentOrder(state, raisedOpen);
  const numbers = numberedAgents(entries.map(({ row }) => row));
  const first = new Map<string, string | null>();
  for (const { row, place } of entries) if (!first.has(row.agent.pane_id)) first.set(row.agent.pane_id, place);
  return (pane: string, place: string | null) => first.has(pane) && first.get(pane) === place ? numberOf(numbers, pane) : null;
}

/** The number ⌥n selects an agent by now, or null past the ninth row. */
export function agentNumber(state: { rest: SnapshotRest | null; agents: AgentRow[] }, paneId: string, raisedOpen: readonly string[] = []): Digit | null {
  return numberOf(numberedAgents(agentListOrder(state, raisedOpen)), paneId);
}
