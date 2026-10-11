// The first-run wizard's rules (PRD first-run-onboarding); `FirstRunWizard.tsx`
// draws them. Which step is on screen is the core's (`ui_state.first_run`);
// everything here reads the snapshot and decides nothing the core owns.

import { AGENT_ADAPTERS } from "./agentAdapters";
import type { ProviderKind } from "./agentPicker";
import type { NotificationPermission } from "./host";
import { FIRST_RUN_VERSION, type AiProvider, type FirstRunStep, type Kit, type KitAgent, type SnapshotRest } from "./snapshot";

/** The steps a wizard shows in order: the desktop app's three, a browser tab's first alone (D-14, B26). */
export type WizardStep = Exclude<FirstRunStep, "done">;

/**
 * The step the first-run wizard shows, or null when it does not show: a Mac
 * that was never asked, a wizard that ended, progress saved by another flow
 * version, or, in a browser tab, any step past the first, which only the
 * desktop app has.
 */
export function firstRunStep(rest: SnapshotRest | null, desktop: boolean): WizardStep | null {
  const run = rest?.ui_state?.first_run;
  if (!run || run.version !== FIRST_RUN_VERSION || run.step === "done") return null;
  if (!desktop && run.step !== "agents") return null;
  return run.step;
}

/** The agents whose sign-in Hide checks (D-06); the others can only say they are installed. */
export const SIGN_IN_CHECKED: ReadonlySet<string> = new Set(["claude-code", "codex", "grok", "pi"]);

/** What a tile says under the agent's name (B8, B9). */
export type TileStatus = "checking" | "ready" | "not_signed_in" | "installed" | "not_installed";

export function tileStatus(agent: KitAgent, providers: readonly AiProvider[] | undefined): TileStatus {
  if (agent.availability !== "available") return "not_installed";
  if (!SIGN_IN_CHECKED.has(agent.id)) return "installed";
  const provider = providers?.find((row) => row.agent === agent.id);
  if (!provider || provider.state === "unread") return "checking";
  if (provider.state === "needs_login") return "not_signed_in";
  // A usage limit is an account that is signed in.
  if (provider.state === "ready" || provider.state === "usage_limited") return "ready";
  return "installed";
}

/** No agent is installed here: the step says so and links the two it names (B9). */
export function noAgentInstalled(agents: readonly KitAgent[]): boolean {
  return agents.every((agent) => agent.availability !== "available");
}

export const SUGGESTED_INSTALLS = ["claude-code", "codex"] as const;

/** One agent's lines under What changes: where Hide writes for it (B11). */
export type ChangeRow = { id: string; label: string; locations: string[] };

/**
 * What Continue changes on this Mac: for each agent left on, the files and
 * folders its pieces are written to (the hook file, the skill folder,
 * Herdr's integration), then the `hide` command's link, as the kit reports
 * them. A piece the agent does not have here (`absent`) writes nothing.
 */
export function whatChanges(agents: readonly KitAgent[], on: ReadonlySet<string>, kit: Kit | null | undefined): { agents: ChangeRow[]; cli: string | null } {
  const rows = agents
    .filter((agent) => on.has(agent.id))
    .map((agent) => ({
      id: agent.id,
      label: agent.label,
      locations: [agent.hook, agent.skill, agent.herdr ?? null].flatMap((piece) => (piece && piece.state !== "absent" && piece.location ? [piece.location] : [])),
    }));
  const cli = kit?.components.find((component) => component.id === "cli")?.location ?? null;
  return { agents: rows, cli };
}

/** A chosen agent whose piece failed to go in, with the first reason the kit gave (B12). */
export type FailedAgent = { id: string; label: string; reason: string | null };

/** The chosen agents that are on and have a failed piece: the step stays and names them (the core keeps it on the same rule). */
export function failedAgents(agents: readonly KitAgent[], chosen: readonly string[]): FailedAgent[] {
  return agents.flatMap((agent) => {
    if (!agent.enabled || !chosen.includes(agent.id)) return [];
    const failed = [agent.hook, agent.skill, agent.herdr ?? null].find((piece) => piece?.state === "failed");
    return failed ? [{ id: agent.id, label: agent.label, reason: failed.reason }] : [];
  });
}

function startKind(id: string): ProviderKind | null {
  const row = AGENT_ADAPTERS.find((adapter) => adapter.id === id && adapter.can_start);
  return row ? (row.herdr_kind as ProviderKind) : null;
}

/**
 * The agent the start panel opens on after the wizard's project (D-15): the
 * first agent that is on and signed in, in the order D-06 checks sign-in,
 * then the first agent that is on. With none on it is null, and no panel
 * opens: the empty Home's Start an agent is the way on (B28).
 */
export function firstStartAgent(agents: readonly KitAgent[], providers: readonly AiProvider[] | undefined): ProviderKind | null {
  const on = agents.filter((agent) => agent.enabled && agent.availability === "available" && startKind(agent.id) !== null);
  const signedIn = on.find((agent) => SIGN_IN_CHECKED.has(agent.id) && tileStatus(agent, providers) === "ready");
  const chosen = signedIn ?? on[0];
  return chosen ? startKind(chosen.id) : null;
}

/**
 * Whether the agent `kind` the start panel would run is one whose sign-in
 * Hide checks and that last read as signed out (B23); an agent Hide cannot
 * check, Terminal, or a sign-in not read yet says nothing (D-06).
 */
export function notSignedIn(kind: ProviderKind | "terminal", providers: readonly AiProvider[] | undefined): boolean {
  const id = AGENT_ADAPTERS.find((adapter) => adapter.can_start && adapter.herdr_kind === kind)?.id;
  if (!id || !SIGN_IN_CHECKED.has(id)) return false;
  return providers?.find((row) => row.agent === id)?.state === "needs_login";
}

/**
 * What the notification step and Settings > General show (B14-B19, B30):
 * `off` before the operator turned it on, `reading` until the first answer
 * about the permission, `asking` while macOS's dialog waits for an answer,
 * `on`, `blocked` when macOS refuses, `unreadable` when Hide could not read
 * macOS's setting, and `on_unchecked` where the system has no permission to
 * read (Windows, Linux). On a Mac that already allows Hide the wizard's step
 * opens on `on` with the switch still off, and its Continue turns the switch
 * on (B16).
 */
export type NotificationState = "off" | "reading" | "asking" | "on" | "blocked" | "unreadable" | "on_unchecked";

export function notificationState(enabled: boolean, permission: NotificationPermission | null, place: "wizard" | "settings"): NotificationState {
  if (permission === "unsupported") return enabled ? "on_unchecked" : "off";
  // macOS's refusal shows even with the switch off: turning it on would do nothing.
  if (permission === "denied") return "blocked";
  if (permission === "granted" && (enabled || place === "wizard")) return "on";
  if (!enabled) return "off";
  if (permission === null) return "reading";
  return permission === "not_determined" ? "asking" : "unreadable";
}

/** How often a screen showing the permission reads it again, so a change in System Settings shows within seconds (B16). */
export const PERMISSION_POLL_MS = 2000;

/** The empty Home (D-16): no project yet, projects but no agent, or the graph whenever it has an agent to draw (B29). */
export type EmptyHome = "no_project" | "no_agent" | null;

export function emptyHome(projects: number, agents: number): EmptyHome {
  if (agents > 0) return null;
  return projects === 0 ? "no_project" : "no_agent";
}

/** The id a notification about an agent carries, so its click and its withdrawal find it again. */
export const AGENT_NOTICE_PREFIX = "agent:";
