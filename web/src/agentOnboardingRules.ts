// The wizard's agent step rules (`FirstRunWizard.tsx` draws them): which
// agents are on, and what Continue sends.
import type { KitAgent } from "./snapshot";

/**
 * Where the tiles start: on a first run every agent installed here is on
 * (`installed`); the step reopened from Help shows the agents switched on
 * now (`enabled`, PRD first-run-onboarding B45).
 */
export type TileStart = "installed" | "enabled";

/**
 * What the tiles show: the start, with the operator's flips (the ids they
 * toggled) turning each the other way. Read from the live availability each
 * time, so an agent that becomes available while the step is open shows as
 * its start says, and Continue sends exactly what is drawn. A tile with no
 * switch is never on.
 */
export function selection(agents: readonly KitAgent[], flipped: ReadonlySet<string>, start: TileStart = "installed"): Set<string> {
  const startsOn = (agent: KitAgent) => start === "installed" || agent.enabled;
  return new Set(agents.filter((agent) => tileSwitchable(agent) && startsOn(agent) !== flipped.has(agent.id)).map((agent) => agent.id));
}

/** A tile has a switch only where the agent is installed here. */
export function tileSwitchable(agent: KitAgent): boolean {
  return agent.availability === "available";
}

/** What Continue sends: the agents on, in the adapters' order. */
export function appliedAgents(agents: readonly KitAgent[], flipped: ReadonlySet<string>, start: TileStart = "installed"): string[] {
  const on = selection(agents, flipped, start);
  return agents.filter((agent) => on.has(agent.id)).map((agent) => agent.id);
}

/**
 * Agents whose Korean reading ends in a consonant (Grok, 그록): the object
 * particle after their name is 을 where every other supported name takes 를.
 */
const KOREAN_FINAL_CONSONANT: ReadonlySet<string> = new Set(["grok"]);

/** The first-run Hide AI line for the agent the rule picked, in the form that agrees with the name's last sound (B46). */
export function hideAiFirstRunKey(agentId: string): "hideAi.firstRun" | "hideAi.firstRunConsonant" {
  return KOREAN_FINAL_CONSONANT.has(agentId) ? "hideAi.firstRunConsonant" : "hideAi.firstRun";
}
