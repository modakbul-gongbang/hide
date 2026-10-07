import { deviceScope, type AgentScope } from "./agentScope";
// What ⌘n and ⌥n select, and the number each tab and agent row carries
// while a hold reveals it (PRD electron-digit-shortcuts-hints D-02). The
// number is the screen order at that moment, first to ninth: the strip's
// tabs left to right, the Agents list's drawn rows top to bottom. A tenth
// item has no number, and a number with nothing at it selects nothing.

import { areasOf } from "./areaLayout";
import type { AgentLayout } from "./agentLayout";
import type { TreeRow } from "./agentRow";
import { frontDeviceId } from "./devices";
import { agentListRows, agentTree, deviceListedAgents, type ListedAgent } from "./navigation";
import type { ProjectRow } from "./projects";
import { DIGITS, type Digit } from "./shortcuts";
import type { AgentRow, Checkout, SnapshotRest, Workspace } from "./snapshot";
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
  rows.slice(0, DIGITS.length).forEach((row, index) => numbered.set(DIGITS[index]!, row.agent.pane_id));
  return numbered;
}

/** The number an item holds, by its id, or null past the ninth. */
export function numberOf(numbered: Map<Digit, string>, id: string): Digit | null {
  for (const [digit, held] of numbered) if (held === id) return digit;
  return null;
}

/** The Agents list's rows in draw order, the list ⌥n numbers whichever list is on screen (B2): the front device's. */
export function agentListOrder(state: { rest: SnapshotRest | null; agents: AgentRow[] }): TreeRow[] {
  return listedAgentOrder(deviceListedAgents(state.rest?.status?.remote, state.rest?.navigator?.devices, state.agents, frontDeviceId(state.rest)), deviceScope(state.rest, frontDeviceId(state.rest)));
}

/** The Agents list's rows in draw order, from the agents it lists. */
export function listedAgentOrder(listed: ListedAgent[], scope: AgentScope | null): TreeRow[] {
  return agentListRows(agentTree(listed, scope));
}

/**
 * Where the Projects list shows each agent's ⌥n number (docs/status-model.md:
 * both appearances share one shortcut, on the first one drawn). ⌥n selects by
 * the Agents list whichever list is on screen, so the number is that one; the
 * Projects list only chooses its one place: the agent's raised row, else its
 * row under the checkout that owns its pane, never a row drawn under a parent
 * in another checkout.
 */
export function projectListNumbers(
  numbered: Map<Digit, string>,
  rows: readonly ProjectRow[],
  workspaces: readonly Workspace[],
): (paneId: string, checkoutId: string | null) => Digit | null {
  // A raised agent folded past its section's cap is not drawn there, so its number stays on its tree row.
  const raised = new Set(rows.flatMap((row) => (row.kind === "raised" ? [...row.agents, ...(row.expanded ? row.more : [])].map(({ agent }) => agent.pane_id) : [])));
  const owners = new Map<string, string>();
  for (const workspace of workspaces) {
    for (const checkout of workspace.checkouts) for (const tab of checkout.tabs) for (const pane of tab.panes) if (!owners.has(pane.id)) owners.set(pane.id, checkout.id);
  }
  return (paneId, checkoutId) => {
    const shown = checkoutId === null ? raised.has(paneId) : !raised.has(paneId) && owners.get(paneId) === checkoutId;
    return shown ? numberOf(numbered, paneId) : null;
  };
}

/** The number ⌥n selects an agent by now, or null past the ninth row. */
export function agentNumber(state: { rest: SnapshotRest | null; agents: AgentRow[] }, paneId: string): Digit | null {
  return numberOf(numberedAgents(agentListOrder(state)), paneId);
}
