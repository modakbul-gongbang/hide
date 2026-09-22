// The close flow, mirrored from the Swift shell (`ShellModel.requestTabClose`,
// `closeCurrentPane`, `ConsequencePolicy`): a pane whose activity is unknown
// is not closed until status is refreshed; a pane with working or attention
// state asks once; an idle pane closes immediately with `confirmed: false`.
// The decision is pure so the tab bar, the pane header and the shortcut
// registry share it and a test can price each branch (PRD S2 B7).

import type { AgentRow, PaneRow } from "./snapshot";

export type CloseKind = "pane" | "tab";

export type CloseDecision =
  | { action: "status_unknown"; label: string }
  | { action: "confirm"; title: string; consequence: string; affected: string[] }
  | { action: "close" };

type Target = { label: string; confirmation: boolean; statusCheck: boolean };

function target(pane: PaneRow, agents: AgentRow[]): Target {
  const agent = agents.find((row) => row.pane_id === pane.id);
  return {
    label: pane.herdr_label ?? pane.id,
    confirmation: pane.requires_close_confirmation || (agent?.requires_close_confirmation ?? false),
    statusCheck: pane.requires_close_status_check || (agent?.requires_close_status_check ?? false),
  };
}

export function closeDecision(kind: CloseKind, panes: PaneRow[], agents: AgentRow[]): CloseDecision {
  const targets = panes.map((pane) => target(pane, agents));
  const unknown = targets.find((row) => row.statusCheck);
  if (unknown) return { action: "status_unknown", label: unknown.label };
  const risky = targets.filter((row) => row.confirmation);
  if (risky.length === 0) return { action: "close" };
  return kind === "pane"
    ? {
        action: "confirm",
        title: "Stop the active pane?",
        consequence: "Closing this pane terminates its running process and interrupts the listed work.",
        affected: risky.map((row) => row.label),
      }
    : {
        action: "confirm",
        title: "Close this tab?",
        consequence: "Closing the tab terminates all listed working or attention panes in one operation.",
        affected: risky.map((row) => row.label),
      };
}

/** The notice the Swift shell shows for an unknown activity status; `refresh_status` is the way out. */
export function statusUnknownNotice(label: string): string {
  return `Activity status for ${label} is unknown. Check status before closing.`;
}
