// Frozen shortcut adapter for tests of the retired two-list sidebar.
import type { AgentScope } from "../src/agentScope";
import type { ProjectRow } from "../src/projects";
import { numberOf } from "../src/numbering";
import type { Digit } from "../src/shortcuts";

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
  scope: AgentScope | null,
): (paneId: string, checkoutId: string | null) => Digit | null {
  // A raised agent folded past its section's cap is not drawn there, so its number stays on its tree row.
  const raised = new Set(rows.flatMap((row) => (row.kind === "raised" ? [...row.agents, ...(row.expanded ? row.more : [])].map(({ agent }) => agent.pane_id) : [])));
  return (paneId, checkoutId) => {
    const shown = checkoutId === null ? raised.has(paneId) : !raised.has(paneId) && scope?.owners[paneId] === checkoutId;
    return shown ? numberOf(numbered, paneId) : null;
  };
}

