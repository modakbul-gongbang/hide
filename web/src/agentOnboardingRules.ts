// The first-run agent choice's rules (the dialog draws them): which agents
// are on, and what Apply sends.
import type { KitAgent } from "./snapshot";

/**
 * What the tiles show: an agent installed on the machine starts on, and the
 * operator's flips (the ids they toggled) turn it off. Read from the live
 * availability each time, so an agent that becomes available while the dialog
 * is open shows on, and Apply sends exactly what is drawn. The operator turns
 * off, never on from nothing.
 */
export function selection(agents: readonly KitAgent[], flipped: ReadonlySet<string>): Set<string> {
  return new Set(agents.filter((agent) => tileSwitchable(agent) && !flipped.has(agent.id)).map((agent) => agent.id));
}

/** A tile has a switch only where the agent is installed here. */
export function tileSwitchable(agent: KitAgent): boolean {
  return agent.availability === "available";
}

/** What Apply sends: the agents still on, in the adapters' order. */
export function appliedAgents(agents: readonly KitAgent[], flipped: ReadonlySet<string>): string[] {
  return agents.filter((agent) => selection(agents, flipped).has(agent.id)).map((agent) => agent.id);
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
