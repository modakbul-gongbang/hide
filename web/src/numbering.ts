// What ⌘n and ⌥n select, and the number each tab and agent row carries
// while a hold reveals it (PRD electron-digit-shortcuts-hints D-02). The
// number is the screen order at that moment, first to ninth: the strip's
// tabs left to right, the Agents list's drawn rows top to bottom. A tenth
// item has no number, and a number with nothing at it selects nothing.

import type { TreeRow } from "./agentRow";
import { DIGITS, type Digit } from "./shortcuts";
import type { Checkout } from "./snapshot";
import { agentEntries } from "./workspace";

/** The tab source id at each number of the strip `checkout` draws, in number order. */
export function numberedTabs(checkout: Checkout): Map<Digit, string> {
  const numbered = new Map<Digit, string>();
  agentEntries(checkout)
    .slice(0, DIGITS.length)
    .forEach((entry, index) => numbered.set(DIGITS[index]!, entry.source_id));
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
