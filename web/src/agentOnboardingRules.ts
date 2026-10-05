// The first-run agent choice's rules (the dialog draws them): which agents
// start on, and what Apply sends.
import type { KitAgent } from "./snapshot";

/** The agents set up on the machine start on: the operator turns off, never on from nothing. */
export function initialSelection(agents: readonly KitAgent[]): Set<string> {
  return new Set(agents.filter((agent) => agent.availability === "available").map((agent) => agent.id));
}

/** A tile has a switch only where the agent is set up here. */
export function tileSwitchable(agent: KitAgent): boolean {
  return agent.availability === "available";
}

/** What Apply sends: the agents still on, in the adapters' order. */
export function appliedAgents(agents: readonly KitAgent[], selection: ReadonlySet<string>): string[] {
  return agents.filter((agent) => tileSwitchable(agent) && selection.has(agent.id)).map((agent) => agent.id);
}
