// What ⌘n and ⌥n select, and the number each tab and agent row carries
// while a hold reveals it (PRD electron-digit-shortcuts-hints D-02). The
// number is the screen order at that moment, first to ninth: the strip's
// tabs left to right, the Agents list's drawn rows top to bottom. A tenth
// item has no number, and a number with nothing at it selects nothing.

import { areasOf } from "./areaLayout";
import type { AgentLayout } from "./agentLayout";
import type { TreeRow } from "./agentRow";
import { DIGITS, type Digit } from "./shortcuts";
import type { Checkout } from "./snapshot";
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
