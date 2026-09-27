// What a sleeping agent's pane says (PRD agent-sleep B10-B14). The core owns
// the state and its plain reason; this only words the header caption and the
// age, so every rule is tested without a browser.

import { relativeActivity } from "./projects";
import type { AgentSleep } from "./snapshot";

/** The pane header's caption beside the moon; a failed wake has no moon. */
export function sleepCaption(sleep: AgentSleep, nowMs: number): { text: string; moon: boolean } {
  if (sleep.state === "waking") return { text: "waking…", moon: true };
  if (sleep.state === "failed") return { text: "could not resume", moon: false };
  const age = relativeActivity(sleep.since_unix_ms, nowMs);
  return { text: age && age !== "now" ? `sleeping · ${age}` : "sleeping", moon: true };
}

/** The line under Waking…: how old the conversation being resumed is. */
export function wakingLine(sleep: AgentSleep, nowMs: number): string {
  const age = relativeActivity(sleep.since_unix_ms, nowMs);
  return age && age !== "now" ? `Resuming the conversation from ${age} ago` : "Resuming the conversation";
}
