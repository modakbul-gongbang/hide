// The Factory secretary (PRD software-factory-ui D-10, B23): an ordinary
// agent tab in this machine's Home whose first prompt says how to read the
// Factory. It holds nothing of its own: it reads the board each time it is
// asked, so closing it loses nothing.

/** The secretary's first prompt; an instruction to an agent, which answers in the person's language. */
export const SECRETARY_PROMPT = [
  "You are the Factory secretary in hide.",
  "Before every answer, read the Factory board with `hide factory status`, and `hide factory show <task>` for one Task; read it again for each new question, since the board moves.",
  "Answer the person's questions about the Factories, their Tasks, what waits on what, and what is waiting for the person.",
  "Do not answer a Task's question for the person, and do not run a command that changes a Factory unless the person asks for it.",
  "Answer in the language the person writes in.",
].join(" ");

import type { SnapshotRest } from "../snapshot";

/** Whether a tab of this machine still holds `paneId`. */
export function paneListed(rest: SnapshotRest | null, paneId: string): boolean {
  return (rest?.navigator?.workspaces ?? []).some((workspace) => workspace.checkouts.some((checkout) => checkout.tabs.some((tab) => tab.panes.some((pane) => pane.id === paneId))));
}

export type SecretaryStart =
  /** Reading the Factory's default runtime. */
  | { phase: "config"; requestId: string }
  /** The Home agent was asked for; its pane answers on `requestId`. */
  | { phase: "start"; requestId: string }
  /** The pane answered; the core keeps it as the secretary once a tab lists it. */
  | { phase: "listing"; paneId: string };
